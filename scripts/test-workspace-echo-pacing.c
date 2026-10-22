// Verify the actual C pacing implementation without opening an X display.
#define main echo_probe_main
#include "probe-workspace-echo.c"
#undef main
#include <assert.h>
int main(void) {
    uint32_t first = UINT32_C(0x6d2b79f5), second = first;
    long left = 0, right = 0, total = 0, previous = 0;
    int varied = 0;
    for (int i = 0; i < 100; i++) {
        long a = inter_sample_pause(i, 1, &first, &left);
        long b = inter_sample_pause(i, 1, &second, &right);
        assert(a == b && a >= 5000000 && a <= 35000000);
        if (i && a != previous) varied = 1;
        if (i % 2) assert(a + previous == 40000000);
        previous = a;
        total += a;
    }
    assert(varied && total == 100L * 20000000);
    for (int i = 0; i < 100; i++) assert(inter_sample_pause(i, 0, &first, &left) == 20000000);
    char *invalid[] = {"probe", "1", "0", "0", "8", "20", "100", "unsupported"};
    assert(echo_probe_main(8, invalid) == 72);
    assert(echo_probe_main(1, invalid) == 71);
    puts("PASS actual C pacing: deterministic sequence, bounds/pair mean, fixed default and invalid CLI rejection");
    return 0;
}
