#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include "latency_core.h"

int main(void) {
    assert(echo_marker(0, 0, 255));
    assert(echo_marker(0x31, 0x31, 0xcd));
    assert(!echo_marker(255, 255, 255) && !echo_marker(0, 0, 0) && !echo_marker(0, 128, 255));

    /* 2x2 image with 4 padding bytes per row; padding holds red bytes that must be ignored. */
    uint8_t image[2 * 12] = {0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 255, 255,
                             0, 0, 0, 255, 0, 0, 250, 255, 0, 0, 255, 255};
    assert(echo_count(image, 2, 2, 12) == 2);

    double values[100];
    for (int i = 0; i < 100; i++) values[i] = 100 - i;
    qsort(values, 100, sizeof values[0], echo_compare);
    assert(echo_percentile(values, 100, 50) == 50 && echo_percentile(values, 100, 95) == 95);
    assert(echo_percentile(values, 1, 95) == 1 && echo_percentile(values, 20, 95) == 19);

    char buffer[64] = {0};
    FILE *out = fmemopen(buffer, sizeof buffer, "w");
    assert(out);
    echo_json_string(out, "a\"b\\c\n");
    fclose(out);
    assert(strcmp(buffer, "\"a\\\"b\\\\c\\u000a\"") == 0);
    puts("latency_core: ok");
    return 0;
}
