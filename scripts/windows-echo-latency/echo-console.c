/* Raw console echo for Windows Terminal: each '1' key paints one pure red two-cell block. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdlib.h>
#include <string.h>

static void put(HANDLE out, const char *text) {
    DWORD written;
    WriteFile(out, text, (DWORD)strlen(text), &written, NULL);
}

int main(int argc, char **argv) {
    long limit = argc > 1 ? strtol(argv[1], NULL, 10) : 0;
    HANDLE in = GetStdHandle(STD_INPUT_HANDLE), out = GetStdHandle(STD_OUTPUT_HANDLE);
    DWORD input_mode, output_mode;
    if (!GetConsoleMode(in, &input_mode) || !GetConsoleMode(out, &output_mode)) return 2;
    SetConsoleMode(in, input_mode & ~(DWORD)(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT));
    SetConsoleMode(out, output_mode | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
    put(out, "\x1b[?25l\x1b[2J\x1b[H");
    long seen = 0;
    char c;
    DWORD n;
    while (ReadFile(in, &c, 1, &n, NULL) && n == 1) {
        if (c == '1') {
            put(out, "\x1b[48;2;255;0;0m  \x1b[0m");
            if (limit && ++seen >= limit) break;
        } else if (c == 'q') {
            break;
        }
    }
    Sleep(1000); /* let the measurement observe the last frame before the tab closes */
    put(out, "\x1b[?25h");
    SetConsoleMode(in, input_mode);
    SetConsoleMode(out, output_mode);
    return 0;
}
