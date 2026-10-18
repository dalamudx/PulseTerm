// Private X11 probe: trusted XTest key injection to first changed glyph pixels.
#define _POSIX_C_SOURCE 200809L
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/keysym.h>
#include <X11/extensions/XTest.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
static uint64_t now_ns(void) {
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t)) exit(70);
    return (uint64_t)t.tv_sec * UINT64_C(1000000000) + (uint64_t)t.tv_nsec;
}
int main(int argc, char **argv) {
    if (argc != 7) return 71;
    Window window = strtoul(argv[1], NULL, 0);
    int x = atoi(argv[2]), y = atoi(argv[3]), width = atoi(argv[4]), height = atoi(argv[5]), count = atoi(argv[6]);
    if (x < 0 || y < 0 || width < 1 || width > 64 || height < 1 || height > 64 || count < 20 || count > 128) return 72;
    Display *display = XOpenDisplay(NULL);
    if (!display) return 73;
    int event, error, major, minor;
    if (!XTestQueryExtension(display, &event, &error, &major, &minor)) return 74;
    XWindowAttributes attributes;
    if (!XGetWindowAttributes(display, window, &attributes) || x + width > attributes.width || y + height > attributes.height) return 75;
    double times[128];
    uint64_t starts[128], observations[128];
    const struct timespec interval = { .tv_sec = 0, .tv_nsec = 500000 };
    const struct timespec pause = { .tv_sec = 0, .tv_nsec = 20000000 };
    for (int i = 0; i < count; i++) {
        XImage *before = XGetImage(display, window, x, y, (unsigned)width, (unsigned)height, AllPlanes, ZPixmap);
        if (!before) return 76;
        KeyCode key = XKeysymToKeycode(display, i % 2 == 0 ? XK_o : XK_x);
        if (!key) return 77;
        uint64_t start = now_ns();
        starts[i] = start;
        if (!XTestFakeKeyEvent(display, key, True, CurrentTime) || !XTestFakeKeyEvent(display, key, False, CurrentTime)) return 78;
        XFlush(display);
        int changed = 0;
        while (now_ns() - start < UINT64_C(2000000000)) {
            XImage *after = XGetImage(display, window, x, y, (unsigned)width, (unsigned)height, AllPlanes, ZPixmap);
            if (!after) return 79;
            changed = memcmp(before->data, after->data, (size_t)before->bytes_per_line * (size_t)before->height) != 0;
            uint64_t observed = now_ns();
            XDestroyImage(after);
            if (changed) { observations[i] = observed; times[i] = (double)(observed - start) / 1000000.0; break; }
            nanosleep(&interval, NULL);
        }
        XDestroyImage(before);
        if (!changed) { fprintf(stderr, "echo.no_glyph_change.%d\n", i); return 80; }
        nanosleep(&pause, NULL);
    }
    XCloseDisplay(display);
    printf("{\"samplesMs\":[");
    for (int i = 0; i < count; i++) printf("%s%.6f", i ? "," : "", times[i]);
    printf("],\"startNs\":[");
    for (int i = 0; i < count; i++) printf("%s%llu", i ? "," : "", (unsigned long long)starts[i]);
    printf("],\"observedNs\":[");
    for (int i = 0; i < count; i++) printf("%s%llu", i ? "," : "", (unsigned long long)observations[i]);
    puts("]}");
    return 0;
}
