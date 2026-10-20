#ifndef PULSE_ECHO_LATENCY_CORE_H
#define PULSE_ECHO_LATENCY_CORE_H
/* Platform-neutral parts of the Windows echo-latency kit, tested natively by build-kit.sh. */
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>

/* Echo targets paint pure #FF0000 cells; the tolerance covers gamma and cell-edge blending. */
static int echo_marker(uint8_t blue, uint8_t green, uint8_t red) {
    return red >= 200 && green <= 60 && blue <= 60;
}

/* BGRA rows; pitch is bytes per row and may exceed width * 4. */
static size_t echo_count(const uint8_t *pixels, unsigned width, unsigned height, unsigned pitch) {
    size_t total = 0;
    for (unsigned y = 0; y < height; y++) {
        const uint8_t *row = pixels + (size_t)y * pitch;
        for (unsigned x = 0; x < width; x++)
            total += (size_t)echo_marker(row[x * 4], row[x * 4 + 1], row[x * 4 + 2]);
    }
    return total;
}

static int echo_compare(const void *left, const void *right) {
    double a = *(const double *)left, b = *(const double *)right;
    return (a > b) - (a < b);
}

/* Nearest-rank percentile of a sorted, non-empty array; integer math avoids 0.95*100 rounding. */
static double echo_percentile(const double *sorted, size_t count, unsigned percent) {
    size_t rank = (count * percent + 99) / 100;
    return sorted[rank ? rank - 1 : 0];
}

/* Writes text (UTF-8) as a JSON string literal. */
static void echo_json_string(FILE *out, const char *text) {
    fputc('"', out);
    for (const unsigned char *c = (const unsigned char *)text; *c; c++) {
        if (*c == '"' || *c == '\\') fprintf(out, "\\%c", *c);
        else if (*c < 0x20) fprintf(out, "\\u%04x", *c);
        else fputc(*c, out);
    }
    fputc('"', out);
}
#endif
