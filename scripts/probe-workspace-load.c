// Fixed native load fixture: real /bin/cat, bounded text, POSIX tty SIGINT.
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <inttypes.h>
#include <limits.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>

static volatile sig_atomic_t interrupted;
static struct timespec signal_time;
static uint64_t nanos(void) {
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now)) _exit(70);
    return (uint64_t)now.tv_sec * UINT64_C(1000000000) + (uint64_t)now.tv_nsec;
}
static void on_interrupt(int number) {
    (void)number;
    int saved = errno;
    clock_gettime(CLOCK_MONOTONIC, &signal_time);
    interrupted = 1;
    errno = saved;
}
static void publish(const char *directory, const char *name, const char *json) {
    char path[PATH_MAX], pending[PATH_MAX];
    if (snprintf(path, sizeof(path), "%s/%s.json", directory, name) >= (int)sizeof(path)
        || snprintf(pending, sizeof(pending), "%s/%s.pending", directory, name) >= (int)sizeof(pending)) _exit(71);
    FILE *file = fopen(pending, "w");
    if (!file || fputs(json, file) < 0 || fclose(file) || rename(pending, path)) _exit(72);
}
int main(void) {
    const char *directory = getenv("PULSE_LOAD_FIXTURE");
    if (!directory) return 73;
    char text[PATH_MAX], json[512];
    if (snprintf(text, sizeof(text), "%s/text-1gib", directory) >= (int)sizeof(text)) return 74;
    struct termios tty;
    if (tcgetattr(STDIN_FILENO, &tty)) return 75;
    tty.c_lflag &= (tcflag_t)~(ECHO | ICANON);
    tty.c_lflag |= ISIG;
    tty.c_iflag &= (tcflag_t)~(IXON | IXOFF);
    tty.c_cc[VINTR] = 3;
    tty.c_cc[VMIN] = 1;
    tty.c_cc[VTIME] = 0;
    if (tcsetattr(STDIN_FILENO, TCSANOW, &tty)) return 76;
    struct sigaction action = {0};
    action.sa_handler = on_interrupt;
    sigemptyset(&action.sa_mask);
    if (sigaction(SIGINT, &action, NULL)) return 77;
    const char *verify = getenv("PULSE_LOAD_VERIFY");
    int verify_load = verify && strcmp(verify, "1") == 0;
    unsigned char commands[3];
    size_t command_count = 0;
    setvbuf(stdout, NULL, _IONBF, 0);
    printf("LOAD READY\r\n");
    for (;;) {
        unsigned char command;
        ssize_t length = read(STDIN_FILENO, &command, 1);
        if (length < 0 && errno == EINTR) continue;
        if (length != 1) return 0;
        if (verify_load) {
            if (command_count >= sizeof(commands)) return 87;
            commands[command_count++] = command;
            if (command == 'Q') {
                if (command_count != 3 || memcmp(commands, "IFQ", 3)) return 87;
                snprintf(json, sizeof(json), "{\"pid\":%ld,\"count\":%zu,\"bytesHex\":\"%02x%02x%02x\"}\n", (long)getpid(), command_count, commands[0], commands[1], commands[2]);
                publish(directory, "load-input-finished", json);
                return 0;
            }
        }
        if (command == 'E') {
            printf("\033[2J\033[HECHO READY\r\nECHO REFERENCE\r\nX");
            char samples[32768] = "[";
            size_t used = 1;
            for (unsigned i = 0; i < 256; i++) {
                unsigned char key;
                ssize_t n = read(STDIN_FILENO, &key, 1);
                uint64_t received = nanos();
                if (n < 0 && errno == EINTR) { i--; continue; }
                if (n != 1) return 83;
                if (key == 'Q') {
                    samples[used++] = ']'; samples[used] = 0;
                    publish(directory, "echo-native", samples);
                    printf("\r\nECHO COMPLETE\r\n"); return 0;
                }
                if (key != 'x' && key != 'o') return 84;
                uint64_t write_started = nanos();
                printf("\033[3;1H%c", key == 'x' ? 'X' : 'O');
                uint64_t written = nanos();
                int added = snprintf(samples + used, sizeof(samples) - used,
                    "%s{\"receivedNs\":%" PRIu64 ",\"writeStartedNs\":%" PRIu64 ",\"writtenNs\":%" PRIu64 "}", i ? "," : "", received, write_started, written);
                if (added < 0 || (size_t)added >= sizeof(samples) - used - 2) return 86;
                used += (size_t)added;
            }
            return 85;
        }
        if (command != 'I' && command != 'F') return 78;
        const char *stage = command == 'I' ? "interrupt" : "full";
        char running[32], finished[32];
        snprintf(running, sizeof(running), "%s-running", stage);
        snprintf(finished, sizeof(finished), "%s-finished", stage);
        interrupted = 0;
        uint64_t start = nanos();
        pid_t child = fork();
        if (child < 0) return 79;
        if (child == 0) {
            signal(SIGINT, SIG_DFL);
            execl("/bin/cat", "cat", text, (char *)NULL);
            _exit(127);
        }
        snprintf(json, sizeof(json), "{\"pid\":%ld,\"startedNs\":%" PRIu64 "}\n", (long)child, start);
        publish(directory, running, json);
        int status;
        while (waitpid(child, &status, 0) < 0) if (errno != EINTR) return 80;
        uint64_t end = nanos();
        int sig = WIFSIGNALED(status) ? WTERMSIG(status) : 0;
        int code = WIFEXITED(status) ? WEXITSTATUS(status) : -1;
        uint64_t received = interrupted ? (uint64_t)signal_time.tv_sec * UINT64_C(1000000000) + (uint64_t)signal_time.tv_nsec : 0;
        snprintf(json, sizeof(json), "{\"pid\":%ld,\"startedNs\":%" PRIu64 ",\"signalNs\":%" PRIu64 ",\"exitedNs\":%" PRIu64 ",\"signal\":%d,\"exitCode\":%d}\n", (long)child, start, received, end, sig, code);
        publish(directory, finished, json);
        if (command == 'I' && (sig != SIGINT || !interrupted)) return 81;
        if (command == 'F' && (sig || code || interrupted)) return 82;
        if (verify_load && command == 'F') {
            // Compare the real tail, not a fresh grid created by ED/HOME.
            printf("LOAD COMPLETE 1073741824\r\n");
        } else {
            printf("\033[2J\033[H%s\r\n", command == 'F' ? "LOAD COMPLETE 1073741824" : "INTERRUPT COMPLETE");
        }
    }
}
