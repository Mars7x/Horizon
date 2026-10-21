// SPDX-FileCopyrightText: Horizon contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// Horizon's libretro log shim. Cores log through a printf-style variadic
// callback, which Rust can't define; this formats each message and hands the
// text to the sink Horizon registered.
#include <stdarg.h>
#include <stdio.h>
#include <string.h>

typedef void (*hz_retro_log_sink)(unsigned level, const char *message);

static hz_retro_log_sink sink;

void hz_retro_log_set_sink(hz_retro_log_sink next) { __atomic_store_n(&sink, next, __ATOMIC_RELEASE); }

void hz_retro_log(unsigned level, const char *format, ...) {
    hz_retro_log_sink target = __atomic_load_n(&sink, __ATOMIC_ACQUIRE);
    if (target == NULL || format == NULL) {
        return;
    }
    char message[1024];
    va_list args;
    va_start(args, format);
    int length = vsnprintf(message, sizeof message, format, args);
    va_end(args);
    if (length < 0) {
        return;
    }
    // Cores end most lines with a newline; the log adds its own.
    size_t end = strnlen(message, sizeof message);
    while (end > 0 && (message[end - 1] == '\n' || message[end - 1] == '\r')) {
        message[--end] = '\0';
    }
    if (end > 0) {
        target(level, message);
    }
}
