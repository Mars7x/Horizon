// Horizon's media shim: FFmpeg decoding behind a few flat functions.
//
// Rust calls these instead of binding FFmpeg's structs directly, so the
// struct layouts stay in C, compiled against the SDK's own headers. The
// runtime ships the libraries (with its codecs-extra extension), so nothing
// is bundled.
//
// One hz_media is used by one thread at a time. A unit returned by
// hz_media_next stays valid until the next call on the same media.
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/channel_layout.h>
#include <libavutil/error.h>
#include <libavutil/mem.h>
#include <libswresample/swresample.h>
#include <libswscale/swscale.h>

enum { HZ_ERROR = -1, HZ_END = 0, HZ_VIDEO = 1, HZ_AUDIO = 2 };

typedef struct {
    int64_t duration_us; // -1 when unknown
    int has_video;
    int has_audio;
} hz_media_info;

typedef struct {
    // Video: packed RGBA (stride == width * 4).
    const uint8_t *pixels;
    int width;
    int height;
    // Audio: interleaved float samples at the requested rate and channels.
    const float *samples;
    int frames;
    int64_t pts_us;      // -1 when unknown
    int64_t duration_us; // -1 when unknown
} hz_media_unit;

typedef struct hz_media {
    AVFormatContext *format;
    AVPacket *packet;
    AVFrame *frame;
    int pending; // `packet` was refused (EAGAIN) and must be sent again

    int video_index;
    AVCodecContext *video;
    int video_done;
    struct SwsContext *scaler;
    uint8_t *pixels;
    size_t pixels_size;
    int box_width, box_height;
    int64_t last_video_pts_us;

    int audio_index;
    AVCodecContext *audio;
    int audio_done;
    SwrContext *resampler;
    int resampler_rate, resampler_format;
    AVChannelLayout resampler_layout;
    float *samples;
    int samples_capacity; // frames
    int out_rate, out_channels;

    int input_done;
    int64_t drop_before_us; // accurate seek: units before this are skipped
} hz_media;

static void set_error(char *error, size_t length, const char *what, int code) {
    if (!error || length == 0) {
        return;
    }
    char reason[AV_ERROR_MAX_STRING_SIZE] = "";
    if (code < 0) {
        av_strerror(code, reason, sizeof reason);
    }
    snprintf(error, length, "%s%s%s", what, reason[0] ? ": " : "", reason);
}

static int64_t to_us(int64_t value, AVRational base) {
    if (value == AV_NOPTS_VALUE) {
        return -1;
    }
    return av_rescale_q(value, base, AV_TIME_BASE_Q);
}

static AVCodecContext *open_decoder(AVStream *stream, const AVCodec *codec, int *error) {
    AVCodecContext *context = avcodec_alloc_context3(codec);
    if (!context) {
        *error = AVERROR(ENOMEM);
        return NULL;
    }
    *error = avcodec_parameters_to_context(context, stream->codecpar);
    if (*error >= 0) {
        context->pkt_timebase = stream->time_base;
        // Decode on every core (frame and slice threads).
        context->thread_count = 0;
        context->thread_type = FF_THREAD_FRAME | FF_THREAD_SLICE;
        *error = avcodec_open2(context, codec, NULL);
    }
    if (*error < 0) {
        avcodec_free_context(&context);
        return NULL;
    }
    return context;
}

void hz_media_close(hz_media *media) {
    if (!media) {
        return;
    }
    avcodec_free_context(&media->video);
    avcodec_free_context(&media->audio);
    sws_freeContext(media->scaler);
    swr_free(&media->resampler);
    av_channel_layout_uninit(&media->resampler_layout);
    av_freep(&media->pixels);
    av_freep(&media->samples);
    av_frame_free(&media->frame);
    av_packet_free(&media->packet);
    avformat_close_input(&media->format);
    av_free(media);
}

// Opens `path`. `box_width` x `box_height` bounds the video frames (scaled
// down, never up, keeping the display aspect ratio). With `want_audio`,
// audio is decoded and resampled to `out_rate` Hz, `out_channels` channels.
hz_media *hz_media_open(const char *path, int box_width, int box_height, int want_audio,
                        int out_rate, int out_channels, char *error, size_t error_length) {
    av_log_set_level(AV_LOG_ERROR);
    hz_media *media = av_mallocz(sizeof *media);
    if (!media) {
        set_error(error, error_length, "out of memory", 0);
        return NULL;
    }
    media->video_index = -1;
    media->audio_index = -1;
    media->box_width = box_width > 0 ? box_width : 1;
    media->box_height = box_height > 0 ? box_height : 1;
    media->out_rate = out_rate;
    media->out_channels = out_channels;
    media->drop_before_us = INT64_MIN;
    media->last_video_pts_us = -1;

    int result = avformat_open_input(&media->format, path, NULL, NULL);
    if (result < 0) {
        set_error(error, error_length, "can't open the file", result);
        hz_media_close(media);
        return NULL;
    }
    result = avformat_find_stream_info(media->format, NULL);
    if (result < 0) {
        set_error(error, error_length, "can't read the streams", result);
        hz_media_close(media);
        return NULL;
    }

    const AVCodec *video_codec = NULL;
    media->video_index =
        av_find_best_stream(media->format, AVMEDIA_TYPE_VIDEO, -1, -1, &video_codec, 0);
    if (media->video_index < 0) {
        set_error(error, error_length, "no playable video stream", media->video_index);
        hz_media_close(media);
        return NULL;
    }
    media->video =
        open_decoder(media->format->streams[media->video_index], video_codec, &result);
    if (!media->video) {
        set_error(error, error_length, "can't decode the video", result);
        hz_media_close(media);
        return NULL;
    }

    if (want_audio && out_rate > 0 && out_channels > 0) {
        const AVCodec *audio_codec = NULL;
        int index = av_find_best_stream(media->format, AVMEDIA_TYPE_AUDIO, -1,
                                        media->video_index, &audio_codec, 0);
        if (index >= 0) {
            media->audio =
                open_decoder(media->format->streams[index], audio_codec, &result);
            // A clip whose sound can't be decoded still plays, silently.
            if (media->audio) {
                media->audio_index = index;
            }
        }
    }

    // Only the chosen streams are read from disk.
    for (unsigned i = 0; i < media->format->nb_streams; i++) {
        if ((int)i != media->video_index && (int)i != media->audio_index) {
            media->format->streams[i]->discard = AVDISCARD_ALL;
        }
    }

    media->packet = av_packet_alloc();
    media->frame = av_frame_alloc();
    if (!media->packet || !media->frame) {
        set_error(error, error_length, "out of memory", 0);
        hz_media_close(media);
        return NULL;
    }
    return media;
}

void hz_media_info_get(const hz_media *media, hz_media_info *info) {
    info->duration_us = media->format->duration == AV_NOPTS_VALUE ? -1 : media->format->duration;
    info->has_video = media->video != NULL;
    info->has_audio = media->audio != NULL;
}

// Seeks to the keyframe at or before `position_us`. With `accurate`, the
// units before `position_us` are then decoded and skipped.
int hz_media_seek(hz_media *media, int64_t position_us, int accurate) {
    int result = avformat_seek_file(media->format, -1, INT64_MIN, position_us, position_us,
                                    AVSEEK_FLAG_BACKWARD);
    if (result < 0) {
        result = av_seek_frame(media->format, -1, position_us, AVSEEK_FLAG_BACKWARD);
    }
    if (media->video) {
        avcodec_flush_buffers(media->video);
    }
    if (media->audio) {
        avcodec_flush_buffers(media->audio);
    }
    if (media->resampler) {
        swr_free(&media->resampler); // drop its buffered tail
    }
    av_packet_unref(media->packet);
    media->pending = 0;
    media->input_done = 0;
    media->video_done = 0;
    media->audio_done = 0;
    media->last_video_pts_us = -1;
    media->drop_before_us = accurate ? position_us : INT64_MIN;
    return result < 0 ? HZ_ERROR : 0;
}

static int fit_size(int width, int height, AVRational aspect, int box_width, int box_height,
                    int *out_width, int *out_height) {
    double display = (double)width;
    if (aspect.num > 0 && aspect.den > 0) {
        display = width * (double)aspect.num / aspect.den;
    }
    double scale = 1.0;
    if (display > box_width) {
        scale = (double)box_width / display;
    }
    if (height * scale > box_height) {
        scale = (double)box_height / height;
    }
    *out_width = (int)(display * scale + 0.5);
    *out_height = (int)(height * scale + 0.5);
    if (*out_width < 1) *out_width = 1;
    if (*out_height < 1) *out_height = 1;
    return 0;
}

static int make_video_unit(hz_media *media, hz_media_unit *unit) {
    AVFrame *frame = media->frame;
    AVStream *stream = media->format->streams[media->video_index];
    int64_t pts = to_us(frame->best_effort_timestamp, stream->time_base);
    int64_t duration = frame->duration > 0 ? to_us(frame->duration, stream->time_base) : -1;
    if (duration < 0) {
        AVRational rate = av_guess_frame_rate(media->format, stream, frame);
        if (rate.num > 0 && rate.den > 0) {
            duration = av_rescale(AV_TIME_BASE, rate.den, rate.num);
        }
    }
    if (pts < 0 && media->last_video_pts_us >= 0 && duration > 0) {
        pts = media->last_video_pts_us + duration;
    }
    media->last_video_pts_us = pts;
    if (media->drop_before_us != INT64_MIN && pts >= 0 &&
        pts + (duration > 0 ? duration / 2 : 0) < media->drop_before_us) {
        return 0; // before an accurate seek's target
    }

    int width, height;
    AVRational aspect = av_guess_sample_aspect_ratio(media->format, stream, frame);
    fit_size(frame->width, frame->height, aspect, media->box_width, media->box_height, &width,
             &height);
    media->scaler = sws_getCachedContext(media->scaler, frame->width, frame->height,
                                         (enum AVPixelFormat)frame->format, width, height,
                                         AV_PIX_FMT_RGBA, SWS_BILINEAR, NULL, NULL, NULL);
    if (!media->scaler) {
        return HZ_ERROR;
    }
    size_t size = (size_t)width * height * 4;
    if (size > media->pixels_size) {
        av_freep(&media->pixels);
        media->pixels = av_malloc(size);
        if (!media->pixels) {
            media->pixels_size = 0;
            return HZ_ERROR;
        }
        media->pixels_size = size;
    }
    uint8_t *planes[4] = {media->pixels, NULL, NULL, NULL};
    int strides[4] = {width * 4, 0, 0, 0};
    sws_scale(media->scaler, (const uint8_t *const *)frame->data, frame->linesize, 0,
              frame->height, planes, strides);

    memset(unit, 0, sizeof *unit);
    unit->pixels = media->pixels;
    unit->width = width;
    unit->height = height;
    unit->pts_us = pts;
    unit->duration_us = duration;
    return HZ_VIDEO;
}

static int make_audio_unit(hz_media *media, hz_media_unit *unit) {
    AVFrame *frame = media->frame;
    AVStream *stream = media->format->streams[media->audio_index];
    int64_t pts = to_us(frame->best_effort_timestamp, stream->time_base);
    int64_t duration = frame->sample_rate > 0
                           ? av_rescale(frame->nb_samples, AV_TIME_BASE, frame->sample_rate)
                           : -1;
    int skip = 0;
    if (media->drop_before_us != INT64_MIN && pts >= 0) {
        if (pts + duration <= media->drop_before_us) {
            return 0;
        }
        if (pts < media->drop_before_us && frame->sample_rate > 0) {
            skip = (int)av_rescale(media->drop_before_us - pts, frame->sample_rate, AV_TIME_BASE);
            pts = media->drop_before_us;
        }
    }

    // (Re)build the resampler when the input format changes.
    if (!media->resampler || media->resampler_rate != frame->sample_rate ||
        media->resampler_format != frame->format ||
        av_channel_layout_compare(&media->resampler_layout, &frame->ch_layout) != 0) {
        swr_free(&media->resampler);
        AVChannelLayout out_layout;
        av_channel_layout_default(&out_layout, media->out_channels);
        int result = swr_alloc_set_opts2(&media->resampler, &out_layout, AV_SAMPLE_FMT_FLT,
                                         media->out_rate, &frame->ch_layout,
                                         (enum AVSampleFormat)frame->format,
                                         frame->sample_rate, 0, NULL);
        av_channel_layout_uninit(&out_layout);
        if (result < 0 || swr_init(media->resampler) < 0) {
            swr_free(&media->resampler);
            return HZ_ERROR;
        }
        media->resampler_rate = frame->sample_rate;
        media->resampler_format = frame->format;
        av_channel_layout_uninit(&media->resampler_layout);
        av_channel_layout_copy(&media->resampler_layout, &frame->ch_layout);
    }

    int capacity = swr_get_out_samples(media->resampler, frame->nb_samples);
    if (capacity <= 0) {
        return 0;
    }
    if (capacity > media->samples_capacity) {
        av_freep(&media->samples);
        media->samples = av_malloc((size_t)capacity * media->out_channels * sizeof(float));
        if (!media->samples) {
            media->samples_capacity = 0;
            return HZ_ERROR;
        }
        media->samples_capacity = capacity;
    }
    uint8_t *out = (uint8_t *)media->samples;
    int frames = swr_convert(media->resampler, &out, capacity,
                             (const uint8_t **)frame->extended_data, frame->nb_samples);
    if (frames < 0) {
        return HZ_ERROR;
    }
    if (skip > 0) {
        int skip_out = (int)av_rescale(skip, media->out_rate, frame->sample_rate);
        if (skip_out >= frames) {
            return 0;
        }
        memmove(media->samples, media->samples + (size_t)skip_out * media->out_channels,
                (size_t)(frames - skip_out) * media->out_channels * sizeof(float));
        frames -= skip_out;
    }
    if (frames == 0) {
        return 0;
    }
    memset(unit, 0, sizeof *unit);
    unit->samples = media->samples;
    unit->frames = frames;
    unit->pts_us = pts;
    unit->duration_us = av_rescale(frames, AV_TIME_BASE, media->out_rate);
    return HZ_AUDIO;
}

// Tries to take one decoded frame from `context`. Returns a unit kind, 0
// when the decoder needs more input, or HZ_ERROR.
static int receive(hz_media *media, AVCodecContext *context, int *done, int video,
                   hz_media_unit *unit) {
    while (context && !*done) {
        int result = avcodec_receive_frame(context, media->frame);
        if (result == AVERROR(EAGAIN)) {
            return 0;
        }
        if (result == AVERROR_EOF) {
            *done = 1;
            return 0;
        }
        if (result < 0) {
            return HZ_ERROR;
        }
        int kind = video ? make_video_unit(media, unit) : make_audio_unit(media, unit);
        av_frame_unref(media->frame);
        if (kind != 0) {
            return kind;
        }
        // Skipped (before a seek target): keep decoding.
    }
    return 0;
}

// Decodes until one unit is ready: HZ_VIDEO, HZ_AUDIO, HZ_END at the end,
// HZ_ERROR on a failure. Units come in decode order, interleaved.
int hz_media_next(hz_media *media, hz_media_unit *unit) {
    for (;;) {
        int kind = receive(media, media->video, &media->video_done, 1, unit);
        if (kind != 0) {
            return kind;
        }
        kind = receive(media, media->audio, &media->audio_done, 0, unit);
        if (kind != 0) {
            return kind;
        }
        if (media->input_done) {
            int video_done = !media->video || media->video_done;
            int audio_done = !media->audio || media->audio_done;
            if (video_done && audio_done) {
                return HZ_END;
            }
            continue;
        }

        if (!media->pending) {
            int result = av_read_frame(media->format, media->packet);
            if (result == AVERROR_EOF ||
                (result < 0 && media->format->pb && avio_feof(media->format->pb))) {
                media->input_done = 1;
                // Drain: the decoders return their last frames, then EOF.
                if (media->video) avcodec_send_packet(media->video, NULL);
                if (media->audio) avcodec_send_packet(media->audio, NULL);
                continue;
            }
            if (result < 0) {
                return HZ_ERROR;
            }
        }
        AVCodecContext *context = NULL;
        if (media->packet->stream_index == media->video_index) {
            context = media->video;
        } else if (media->packet->stream_index == media->audio_index) {
            context = media->audio;
        }
        if (!context) {
            av_packet_unref(media->packet);
            media->pending = 0;
            continue;
        }
        int result = avcodec_send_packet(context, media->packet);
        if (result == AVERROR(EAGAIN)) {
            // The decoder is full: take its frames first, then resend.
            media->pending = 1;
            continue;
        }
        av_packet_unref(media->packet);
        media->pending = 0;
        if (result < 0 && result != AVERROR_INVALIDDATA) {
            return HZ_ERROR;
        }
    }
}
