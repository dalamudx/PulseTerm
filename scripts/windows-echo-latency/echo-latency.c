/* Keypress-to-present latency for the foreground window: SendInput a '1' key, then watch DXGI
 * desktop duplication until red marker pixels increase, and use that frame's QPC present time. */
#define WIN32_LEAN_AND_MEAN
#define COBJMACROS
#define WINVER 0x0A00
#define _WIN32_WINNT 0x0A00
#include <windows.h>
#include <initguid.h>
#include <d3d11.h>
#include <dxgi1_2.h>
#include <stdlib.h>
#include <string.h>
#include "latency_core.h"

#define MAX_SAMPLES 512
#define MIN_MARKER_INCREASE 16

static ID3D11DeviceContext *context;
static IDXGIOutputDuplication *duplication;
static ID3D11Texture2D *staging;
static D3D11_BOX box;
static size_t markers;
static unsigned accumulated;
static LARGE_INTEGER frequency;

static void fail(int code, const char *message) {
    fprintf(stderr, "echo-latency: %s\n", message);
    exit(code);
}

static double ms(LONGLONG ticks) { return (double)ticks * 1000.0 / (double)frequency.QuadPart; }

static void utf8(const wchar_t *text, char *buffer, int size) {
    if (!WideCharToMultiByte(CP_UTF8, 0, text, -1, buffer, size, NULL, NULL)) buffer[0] = 0;
}

/* Returns 1 and the frame's QPC present time (0 when only the pointer moved), or 0 on timeout. */
static int acquire(UINT timeout, LONGLONG *present) {
    DXGI_OUTDUPL_FRAME_INFO info;
    IDXGIResource *resource = NULL;
    HRESULT hr = IDXGIOutputDuplication_AcquireNextFrame(duplication, timeout, &info, &resource);
    if (hr == DXGI_ERROR_WAIT_TIMEOUT) return 0;
    if (FAILED(hr)) fail(5, "duplication.acquire_failed (display mode, UAC or lock screen changed)");
    ID3D11Texture2D *texture = NULL;
    if (FAILED(IDXGIResource_QueryInterface(resource, &IID_ID3D11Texture2D, (void **)&texture)))
        fail(5, "duplication.texture");
    ID3D11DeviceContext_CopySubresourceRegion(context, (ID3D11Resource *)staging, 0, 0, 0, 0,
                                              (ID3D11Resource *)texture, 0, &box);
    D3D11_MAPPED_SUBRESOURCE mapped;
    if (FAILED(ID3D11DeviceContext_Map(context, (ID3D11Resource *)staging, 0, D3D11_MAP_READ, 0, &mapped)))
        fail(5, "staging.map");
    markers = echo_count(mapped.pData, box.right - box.left, box.bottom - box.top, mapped.RowPitch);
    ID3D11DeviceContext_Unmap(context, (ID3D11Resource *)staging, 0);
    ID3D11Texture2D_Release(texture);
    IDXGIResource_Release(resource);
    if (info.AccumulatedFrames > 1) accumulated++;
    *present = info.LastPresentTime.QuadPart;
    IDXGIOutputDuplication_ReleaseFrame(duplication);
    return 1;
}

static RECT client_rect(HWND window) {
    RECT rect;
    POINT origin = {0, 0};
    if (!GetClientRect(window, &rect) || !ClientToScreen(window, &origin)) fail(3, "window.rect");
    OffsetRect(&rect, origin.x, origin.y);
    return rect;
}

static void write_series(FILE *out, const char *name, const double *values, int count) {
    fprintf(out, ",\n  ");
    echo_json_string(out, name);
    fprintf(out, ": [");
    for (int i = 0; i < count; i++) fprintf(out, "%s%.3f", i ? "," : "", values[i]);
    fprintf(out, "]");
}

int main(int argc, char **argv) {
    const char *label = NULL, *expect = NULL, *title_prefix = NULL, *output = NULL;
    int samples = 100, warmup = 5, countdown = 5;
    for (int i = 1; i + 1 < argc; i += 2) {
        if (!strcmp(argv[i], "--label")) label = argv[i + 1];
        else if (!strcmp(argv[i], "--expect")) expect = argv[i + 1];
        else if (!strcmp(argv[i], "--title")) title_prefix = argv[i + 1];
        else if (!strcmp(argv[i], "--out")) output = argv[i + 1];
        else if (!strcmp(argv[i], "--samples")) samples = atoi(argv[i + 1]);
        else if (!strcmp(argv[i], "--warmup")) warmup = atoi(argv[i + 1]);
        else if (!strcmp(argv[i], "--countdown")) countdown = atoi(argv[i + 1]);
        else fail(1, "usage: echo-latency --label L --expect EXE --title PREFIX --out FILE [--samples N --warmup N --countdown S]");
    }
    if (!label || !expect || !title_prefix || !output || samples < 20 || samples > MAX_SAMPLES || warmup < 0 ||
        warmup > 50 || countdown < 0 || countdown > 60)
        fail(1, "usage: echo-latency --label L --expect EXE --title PREFIX --out FILE [--samples 20..512 --warmup N --countdown S]");
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    QueryPerformanceFrequency(&frequency);
    srand(GetTickCount());

    fprintf(stderr, "echo-latency: %s starts in %d s; do not touch keyboard or mouse\n", label, countdown);
    Sleep((DWORD)countdown * 1000);
    HWND window = GetForegroundWindow();
    DWORD pid = 0, thread = GetWindowThreadProcessId(window, &pid);
    wchar_t wide[MAX_PATH] = L"";
    char process[MAX_PATH * 3] = "", title[512] = "";
    DWORD length = MAX_PATH;
    HANDLE handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
    if (!handle || !QueryFullProcessImageNameW(handle, 0, wide, &length)) fail(2, "target.process_unreadable");
    CloseHandle(handle);
    const wchar_t *base = wcsrchr(wide, L'\\');
    utf8(base ? base + 1 : wide, process, sizeof process);
    GetWindowTextW(window, wide, MAX_PATH);
    utf8(wide, title, sizeof title);
    if (_stricmp(process, expect) || strncmp(title, title_prefix, strlen(title_prefix))) {
        fprintf(stderr, "echo-latency: foreground is %s \"%s\", expected %s \"%s...\"\n", process, title, expect,
                title_prefix);
        return 2;
    }
    HKL layout = GetKeyboardLayout(thread);
    if (PRIMARYLANGID(LOWORD((UINT_PTR)layout)) == LANG_CHINESE)
        fprintf(stderr, "echo-latency: Chinese keyboard layout active; IME must be in English mode\n");

    RECT region = client_rect(window);
    HMONITOR monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);
    IDXGIFactory1 *factory = NULL;
    if (FAILED(CreateDXGIFactory1(&IID_IDXGIFactory1, (void **)&factory))) fail(5, "dxgi.factory");
    IDXGIAdapter1 *adapter = NULL;
    IDXGIOutput *found = NULL;
    DXGI_OUTPUT_DESC output_desc;
    for (UINT a = 0; !found && IDXGIFactory1_EnumAdapters1(factory, a, &adapter) == S_OK; a++) {
        IDXGIOutput *candidate = NULL;
        for (UINT o = 0; !found && IDXGIAdapter1_EnumOutputs(adapter, o, &candidate) == S_OK; o++) {
            IDXGIOutput_GetDesc(candidate, &output_desc);
            if (output_desc.Monitor == monitor) found = candidate;
            else IDXGIOutput_Release(candidate);
        }
        if (!found) IDXGIAdapter1_Release(adapter);
    }
    if (!found) fail(5, "dxgi.output_not_found");
    RECT desk = output_desc.DesktopCoordinates;
    if (region.left < desk.left || region.top < desk.top || region.right > desk.right || region.bottom > desk.bottom ||
        region.right <= region.left || region.bottom <= region.top)
        fail(3, "window.must_be_fully_on_one_monitor");
    ID3D11Device *device = NULL;
    if (FAILED(D3D11CreateDevice((IDXGIAdapter *)adapter, D3D_DRIVER_TYPE_UNKNOWN, NULL, 0, NULL, 0,
                                 D3D11_SDK_VERSION, &device, NULL, &context)))
        fail(5, "d3d11.device");
    IDXGIOutput1 *output1 = NULL;
    if (FAILED(IDXGIOutput_QueryInterface(found, &IID_IDXGIOutput1, (void **)&output1)) ||
        FAILED(IDXGIOutput1_DuplicateOutput(output1, (IUnknown *)device, &duplication)))
        fail(5, "dxgi.duplicate_output (close other screen recorders; HDR must be off)");
    DXGI_OUTDUPL_DESC dup_desc;
    IDXGIOutputDuplication_GetDesc(duplication, &dup_desc);
    if (dup_desc.ModeDesc.Format != DXGI_FORMAT_B8G8R8A8_UNORM) fail(5, "dxgi.format_not_bgra8 (turn HDR off)");
    if (dup_desc.Rotation != DXGI_MODE_ROTATION_IDENTITY) fail(5, "dxgi.rotated_display_unsupported");
    box = (D3D11_BOX){(UINT)(region.left - desk.left), (UINT)(region.top - desk.top), 0,
                      (UINT)(region.right - desk.left), (UINT)(region.bottom - desk.top), 1};
    D3D11_TEXTURE2D_DESC texture_desc = {box.right - box.left, box.bottom - box.top, 1, 1, DXGI_FORMAT_B8G8R8A8_UNORM,
                                         {1, 0}, D3D11_USAGE_STAGING, 0, D3D11_CPU_ACCESS_READ, 0};
    if (FAILED(ID3D11Device_CreateTexture2D(device, &texture_desc, NULL, &staging))) fail(5, "d3d11.staging");

    LONGLONG present = 0;
    if (!acquire(1000, &present)) fail(5, "duplication.first_frame_timeout");
    static double latency[MAX_SAMPLES], acquired[MAX_SAMPLES], increase[MAX_SAMPLES];
    unsigned missing_present = 0;
    UINT scan = MapVirtualKeyW('1', MAPVK_VK_TO_VSC);
    for (int i = 0; i < warmup + samples; i++) {
        RECT now_region = client_rect(window);
        if (GetForegroundWindow() != window) fail(3, "focus.lost (do not touch keyboard or mouse)");
        if (!EqualRect(&now_region, &region)) fail(3, "window.moved_or_resized");
        while (acquire(0, &present)) {
        }
        size_t baseline = markers;
        INPUT input[2];
        memset(input, 0, sizeof input);
        input[0].type = input[1].type = INPUT_KEYBOARD;
        input[0].ki.wVk = input[1].ki.wVk = '1';
        input[0].ki.wScan = input[1].ki.wScan = (WORD)scan;
        input[1].ki.dwFlags = KEYEVENTF_KEYUP;
        LARGE_INTEGER start, now;
        QueryPerformanceCounter(&start);
        if (SendInput(2, input, sizeof(INPUT)) != 2) fail(6, "input.send_failed");
        for (;;) {
            QueryPerformanceCounter(&now);
            double waited = ms(now.QuadPart - start.QuadPart);
            if (waited > 1000) {
                fprintf(stderr, "echo-latency: sample %d produced no red marker within 1s (IME in Chinese mode?)\n", i);
                return 4;
            }
            if (!acquire((UINT)(1000 - waited) + 1, &present) || markers < baseline + MIN_MARKER_INCREASE) continue;
            QueryPerformanceCounter(&now);
            break;
        }
        if (present && present < start.QuadPart) fail(4, "present.before_input");
        if (!present) missing_present++;
        if (i >= warmup) {
            int k = i - warmup;
            acquired[k] = ms(now.QuadPart - start.QuadPart);
            latency[k] = present ? ms(present - start.QuadPart) : acquired[k];
            increase[k] = (double)(markers - baseline);
        }
        Sleep(40 + (DWORD)(rand() % 50)); /* decorrelate key timing from the vsync phase */
    }

    double sorted[MAX_SAMPLES], sorted_increase[MAX_SAMPLES];
    memcpy(sorted, latency, sizeof(double) * (size_t)samples);
    memcpy(sorted_increase, increase, sizeof(double) * (size_t)samples);
    qsort(sorted, (size_t)samples, sizeof(double), echo_compare);
    qsort(sorted_increase, (size_t)samples, sizeof(double), echo_compare);
    DXGI_ADAPTER_DESC1 adapter_desc;
    IDXGIAdapter1_GetDesc1(adapter, &adapter_desc);
    char adapter_name[512];
    utf8(adapter_desc.Description, adapter_name, sizeof adapter_name);
    DEVMODEW mode = {.dmSize = sizeof(DEVMODEW)};
    EnumDisplaySettingsW(output_desc.DeviceName, ENUM_CURRENT_SETTINGS, &mode);
    RTL_OSVERSIONINFOW version = {.dwOSVersionInfoSize = sizeof version};
    typedef LONG(WINAPI * RtlGetVersionFn)(RTL_OSVERSIONINFOW *);
    RtlGetVersionFn get_version = (RtlGetVersionFn)(void *)GetProcAddress(GetModuleHandleW(L"ntdll.dll"), "RtlGetVersion");
    if (get_version) get_version(&version);
    DXGI_RATIONAL rate = dup_desc.ModeDesc.RefreshRate;

    FILE *out = fopen(output, "w");
    if (!out) fail(7, "output.open");
    fprintf(out, "{\n  \"schemaVersion\": 1,\n  \"label\": ");
    echo_json_string(out, label);
    fprintf(out, ",\n  \"process\": ");
    echo_json_string(out, process);
    fprintf(out, ",\n  \"title\": ");
    echo_json_string(out, title);
    fprintf(out, ",\n  \"adapter\": ");
    echo_json_string(out, adapter_name);
    fprintf(out,
            ",\n  \"windowsBuild\": \"%lu.%lu.%lu\",\n  \"refreshHz\": %.3f,\n  \"displayFrequency\": %lu,\n"
            "  \"outputWidth\": %u,\n  \"outputHeight\": %u,\n  \"regionWidth\": %u,\n  \"regionHeight\": %u,\n"
            "  \"windowDpi\": %u,\n  \"keyboardLayout\": \"%08llx\",\n  \"samples\": %d,\n  \"warmup\": %d,\n"
            "  \"accumulatedFrameEvents\": %u,\n  \"presentTimeMissing\": %u,\n  \"p50Ms\": %.3f,\n  \"p95Ms\": %.3f,\n"
            "  \"minMs\": %.3f,\n  \"maxMs\": %.3f,\n  \"medianMarkerIncrease\": %.0f",
            version.dwMajorVersion, version.dwMinorVersion, version.dwBuildNumber,
            rate.Denominator ? (double)rate.Numerator / rate.Denominator : 0.0, mode.dmDisplayFrequency,
            dup_desc.ModeDesc.Width, dup_desc.ModeDesc.Height, box.right - box.left, box.bottom - box.top,
            GetDpiForWindow(window), (unsigned long long)(UINT_PTR)layout, samples, warmup, accumulated,
            missing_present, echo_percentile(sorted, (size_t)samples, 50), echo_percentile(sorted, (size_t)samples, 95),
            sorted[0], sorted[samples - 1], echo_percentile(sorted_increase, (size_t)samples, 50));
    write_series(out, "latencyMs", latency, samples);
    write_series(out, "acquiredMs", acquired, samples);
    fprintf(out, "\n}\n");
    if (fclose(out)) fail(7, "output.write");
    printf("%s: p50 %.2f ms, p95 %.2f ms (%d samples, %.0f Hz)\n", label,
           echo_percentile(sorted, (size_t)samples, 50), echo_percentile(sorted, (size_t)samples, 95), samples,
           rate.Denominator ? (double)rate.Numerator / rate.Denominator : 0.0);
    return 0;
}
