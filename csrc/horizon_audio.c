// Horizon's audio shim: one PipeWire playback stream behind flat functions.
//
// Each stream runs its own PipeWire thread loop. PipeWire calls `fill` on
// its real-time thread for every buffer it wants, with the interleaved float
// samples to write and the output latency (how long until the first of them
// is heard). Opening and closing never wait for the server to
// answer, so a stalled server can't freeze the caller.
#include <errno.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include <pipewire/pipewire.h>
#include <spa/param/audio/format-utils.h>

typedef void (*hz_audio_fill)(void *user, float *samples, uint32_t count, int64_t delay_ns);

typedef struct hz_audio {
    struct pw_thread_loop *loop;
    struct pw_stream *stream;
    struct spa_hook listener;
    hz_audio_fill fill;
    void *user;
    uint32_t channels;
} hz_audio;

static pthread_once_t initialized = PTHREAD_ONCE_INIT;

static void initialize(void) { pw_init(NULL, NULL); }

static void on_process(void *data) {
    hz_audio *audio = data;
    struct pw_buffer *buffer = pw_stream_dequeue_buffer(audio->stream);
    if (!buffer) {
        return;
    }
    struct spa_data *plane = &buffer->buffer->datas[0];
    float *samples = plane->data;
    if (!samples) {
        pw_stream_queue_buffer(audio->stream, buffer);
        return;
    }
    uint32_t stride = sizeof(float) * audio->channels;
    uint32_t frames = plane->maxsize / stride;
    if (buffer->requested > 0 && buffer->requested < frames) {
        frames = (uint32_t)buffer->requested;
    }
    int64_t delay_ns = 0;
    struct pw_time time;
    if (pw_stream_get_time_n(audio->stream, &time, sizeof time) == 0 && time.rate.denom > 0) {
        delay_ns = time.delay * SPA_NSEC_PER_SEC * (int64_t)time.rate.num / time.rate.denom;
    }
    audio->fill(audio->user, samples, frames * audio->channels, delay_ns);
    plane->chunk->offset = 0;
    plane->chunk->stride = (int32_t)stride;
    plane->chunk->size = frames * stride;
    pw_stream_queue_buffer(audio->stream, buffer);
}

static const struct pw_stream_events stream_events = {
    PW_VERSION_STREAM_EVENTS,
    .process = on_process,
};

void hz_audio_close(hz_audio *audio) {
    if (!audio) {
        return;
    }
    if (audio->loop) {
        pw_thread_loop_lock(audio->loop);
        if (audio->stream) {
            pw_stream_destroy(audio->stream);
        }
        pw_thread_loop_unlock(audio->loop);
        pw_thread_loop_stop(audio->loop);
        pw_thread_loop_destroy(audio->loop);
    }
    free(audio);
}

// Opens a playback stream of `channels` x float at `rate` Hz, asking the
// graph for `latency_frames` per cycle. `role` is the PipeWire media role
// ("Game", "Movie"…). Returns NULL (and an error) when PipeWire is missing.
hz_audio *hz_audio_open(const char *name, const char *role, uint32_t rate, uint32_t channels,
                        uint32_t latency_frames, hz_audio_fill fill, void *user, char *error,
                        size_t error_length) {
    pthread_once(&initialized, initialize);
    hz_audio *audio = calloc(1, sizeof *audio);
    if (!audio) {
        snprintf(error, error_length, "out of memory");
        return NULL;
    }
    audio->fill = fill;
    audio->user = user;
    audio->channels = channels;

    audio->loop = pw_thread_loop_new("horizon-audio", NULL);
    if (!audio->loop || pw_thread_loop_start(audio->loop) < 0) {
        snprintf(error, error_length, "PipeWire thread did not start");
        hz_audio_close(audio);
        return NULL;
    }

    char latency[32];
    snprintf(latency, sizeof latency, "%u/%u", latency_frames, rate);
    struct pw_properties *props = pw_properties_new(
        PW_KEY_MEDIA_TYPE, "Audio", PW_KEY_MEDIA_CATEGORY, "Playback", PW_KEY_MEDIA_ROLE,
        role, PW_KEY_NODE_LATENCY, latency, PW_KEY_APP_NAME, "Horizon", NULL);

    pw_thread_loop_lock(audio->loop);
    audio->stream = pw_stream_new_simple(pw_thread_loop_get_loop(audio->loop), name, props,
                                         &stream_events, audio);
    if (!audio->stream) {
        pw_thread_loop_unlock(audio->loop);
        snprintf(error, error_length, "PipeWire is not available: %s", strerror(errno));
        hz_audio_close(audio);
        return NULL;
    }

    uint8_t pod[1024];
    struct spa_pod_builder builder = SPA_POD_BUILDER_INIT(pod, sizeof pod);
    struct spa_audio_info_raw info = {
        .format = SPA_AUDIO_FORMAT_F32,
        .rate = rate,
        .channels = channels,
    };
    if (channels == 1) {
        info.position[0] = SPA_AUDIO_CHANNEL_MONO;
    } else if (channels >= 2) {
        info.position[0] = SPA_AUDIO_CHANNEL_FL;
        info.position[1] = SPA_AUDIO_CHANNEL_FR;
    }
    const struct spa_pod *params[1] = {
        spa_format_audio_raw_build(&builder, SPA_PARAM_EnumFormat, &info),
    };
    int result = pw_stream_connect(audio->stream, PW_DIRECTION_OUTPUT, PW_ID_ANY,
                                   PW_STREAM_FLAG_AUTOCONNECT | PW_STREAM_FLAG_MAP_BUFFERS |
                                       PW_STREAM_FLAG_RT_PROCESS,
                                   params, 1);
    pw_thread_loop_unlock(audio->loop);
    if (result < 0) {
        snprintf(error, error_length, "PipeWire stream did not connect: %s", strerror(-result));
        hz_audio_close(audio);
        return NULL;
    }
    return audio;
}

// Starts or stops asking for samples. A stopped stream costs nothing and
// lets the output device sleep.
void hz_audio_set_active(hz_audio *audio, int active) {
    pw_thread_loop_lock(audio->loop);
    pw_stream_set_active(audio->stream, active != 0);
    pw_thread_loop_unlock(audio->loop);
}
