// SPDX-FileCopyrightText: Horizon contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#include <errno.h>
#include <stdint.h>
#include <unistd.h>
#include <jxl/encode.h>

typedef struct {
    const uint8_t *rgba;
    size_t length;
    uint32_t duration_ms;
} HorizonJxlFrame;

/* Encode caller-owned RGBA8 as lossless sRGB frames. The descriptor is
 * borrowed, never closed. A small output buffer avoids a second image-sized
 * allocation. No parallel runner: this work already runs on a worker. */
int horizon_jxl_write(int fd, const HorizonJxlFrame *frames, size_t count,
                      uint32_t width, uint32_t height) {
    if (!frames || !count || !width || !height || width > SIZE_MAX / height / 4)
        return EINVAL;
    for (size_t i = 0; i < count; i++) {
        if (!frames[i].rgba || frames[i].length != (size_t)width * height * 4 ||
            (count > 1 && !frames[i].duration_ms)) return EINVAL;
    }
    JxlEncoder *encoder = JxlEncoderCreate(NULL);
    if (!encoder) return ENOMEM;
    int result = EIO;
    JxlBasicInfo info;
    JxlEncoderInitBasicInfo(&info);
    info.xsize = width;
    info.ysize = height;
    info.bits_per_sample = 8;
    info.num_color_channels = 3;
    info.num_extra_channels = 1;
    info.alpha_bits = 8;
    info.uses_original_profile = JXL_TRUE;
    info.have_animation = count > 1;
    info.animation.tps_numerator = 1000;
    info.animation.tps_denominator = 1;
    info.animation.num_loops = 0;
    if (count > 1 && JxlEncoderSetCodestreamLevel(encoder, 10) != JXL_ENC_SUCCESS)
        goto done;
    if (JxlEncoderSetBasicInfo(encoder, &info) != JXL_ENC_SUCCESS) goto done;
    JxlColorEncoding color;
    JxlColorEncodingSetToSRGB(&color, JXL_FALSE);
    if (JxlEncoderSetColorEncoding(encoder, &color) != JXL_ENC_SUCCESS) goto done;
    JxlEncoderFrameSettings *settings = JxlEncoderFrameSettingsCreate(encoder, NULL);
    if (!settings ||
        JxlEncoderSetFrameLossless(settings, JXL_TRUE) != JXL_ENC_SUCCESS ||
        JxlEncoderSetExtraChannelDistance(settings, 0, 0) != JXL_ENC_SUCCESS ||
        JxlEncoderFrameSettingsSetOption(settings, JXL_ENC_FRAME_SETTING_EFFORT, 7)
            != JXL_ENC_SUCCESS) goto done;
    JxlPixelFormat format = {4, JXL_TYPE_UINT8, JXL_NATIVE_ENDIAN, 0};
    for (size_t i = 0; i < count; i++) {
        if (count > 1) {
            JxlFrameHeader header;
            JxlEncoderInitFrameHeader(&header);
            header.duration = frames[i].duration_ms;
            if (JxlEncoderSetFrameHeader(settings, &header) != JXL_ENC_SUCCESS)
                goto done;
        }
        if (JxlEncoderAddImageFrame(settings, &format, frames[i].rgba,
                                   frames[i].length) != JXL_ENC_SUCCESS) goto done;
        if (i + 1 == count) JxlEncoderCloseInput(encoder);
        JxlEncoderStatus status;
        do {
            uint8_t buffer[65536];
            uint8_t *next = buffer;
            size_t available = sizeof(buffer);
            status = JxlEncoderProcessOutput(encoder, &next, &available);
            if (status != JXL_ENC_SUCCESS && status != JXL_ENC_NEED_MORE_OUTPUT) goto done;
            size_t remaining = sizeof(buffer) - available;
            const uint8_t *bytes = buffer;
            while (remaining) {
                ssize_t written = write(fd, bytes, remaining);
                if (written < 0 && errno == EINTR) continue;
                if (written <= 0) {
                    result = written < 0 ? errno : EIO;
                    goto done;
                }
                bytes += written;
                remaining -= (size_t)written;
            }
        } while (status == JXL_ENC_NEED_MORE_OUTPUT);
    }
    result = 0;
done:
    JxlEncoderDestroy(encoder);
    return result;
}
