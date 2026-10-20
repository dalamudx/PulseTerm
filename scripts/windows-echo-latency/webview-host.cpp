// Minimal native WebView2 host for the local echo page. No Tauri, PTY or product IPC.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <shellapi.h>
#include <WebView2.h>
#include <atomic>
#include <functional>
#include <string>
#include <cstdio>

static HWND window;
static ICoreWebView2Environment *environment;
static ICoreWebView2Controller *controller;
static ICoreWebView2 *webview;
static FILE *log_file;
static int exit_code;
static std::wstring uri;

#ifdef __MINGW32__
// The SDK's MIDL uuid attributes are MSVC-only; bind the same published interface IDs.
__CRT_UUID_DECL(ICoreWebView2DocumentTitleChangedEventHandler, 0xf5f2b923, 0x953e, 0x4042, 0x9f,0x95,0xf3,0xa1,0x18,0xe1,0xaf,0xd4)
__CRT_UUID_DECL(ICoreWebView2NavigationCompletedEventHandler, 0xd33a35bf, 0x1c49, 0x4f98, 0x93,0xab,0x00,0x6e,0x05,0x33,0xfe,0x1c)
__CRT_UUID_DECL(ICoreWebView2CreateCoreWebView2ControllerCompletedHandler, 0x6c4819f3, 0xc9b7, 0x4260, 0x81,0x27,0xc9,0xf5,0xbd,0xe7,0xf6,0x8c)
__CRT_UUID_DECL(ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler, 0x4e8a3389, 0xc9d8, 0x4bd2, 0xb6,0xb5,0x12,0x4f,0xee,0x6c,0xc1,0x4d)
#endif

static void check(HRESULT result, const char *stage) {
    if (SUCCEEDED(result)) return;
    if (log_file) { std::fprintf(log_file, "%s failed: HRESULT 0x%08lx\n", stage, static_cast<unsigned long>(result)); std::fflush(log_file); }
    exit_code = 2;
    PostMessageW(window, WM_CLOSE, 0, 0);
}

template<class Interface, class... Args>
class Handler final : public Interface {
    std::atomic<ULONG> references{1};
    std::function<HRESULT(Args...)> callback;
public:
    explicit Handler(std::function<HRESULT(Args...)> fn) : callback(std::move(fn)) {}
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void **out) override {
        if (!out) return E_POINTER;
        *out = nullptr;
        if (IsEqualIID(iid, IID_IUnknown) || IsEqualIID(iid, __uuidof(Interface))) {
            *out = static_cast<Interface *>(this); AddRef(); return S_OK;
        }
        return E_NOINTERFACE;
    }
    ULONG STDMETHODCALLTYPE AddRef() override { return ++references; }
    ULONG STDMETHODCALLTYPE Release() override {
        ULONG remaining = --references;
        if (!remaining) delete this;
        return remaining;
    }
    HRESULT STDMETHODCALLTYPE Invoke(Args... args) override { return callback(args...); }
};

static LRESULT CALLBACK events(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam) {
    if (message == WM_SIZE && controller) {
        RECT bounds; GetClientRect(hwnd, &bounds); controller->put_Bounds(bounds);
    } else if (message == WM_MOVE && controller) {
        controller->NotifyParentWindowPositionChanged();
    } else if (message == WM_SETFOCUS && controller) {
        controller->MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
    } else if (message == WM_CLOSE) {
        if (controller) controller->Close();
        DestroyWindow(hwnd);
        return 0;
    } else if (message == WM_DESTROY) {
        PostQuitMessage(exit_code);
        return 0;
    }
    return DefWindowProcW(hwnd, message, wparam, lparam);
}

int WINAPI wWinMain(HINSTANCE instance, HINSTANCE, PWSTR, int show) {
    int argc = 0;
    wchar_t **argv = CommandLineToArgvW(GetCommandLineW(), &argc);
    std::wstring profile, runtime, log_path;
    for (int i = 1; i + 1 < argc; i += 2) {
        const std::wstring flag = argv[i];
        if (flag == L"--url") uri = argv[i + 1];
        else if (flag == L"--profile") profile = argv[i + 1];
        else if (flag == L"--runtime") runtime = argv[i + 1];
        else if (flag == L"--log") log_path = argv[i + 1];
        else { LocalFree(argv); return 1; }
    }
    LocalFree(argv);
    if (uri.rfind(L"file:///", 0) != 0 || profile.empty() || log_path.empty()) return 1;
    log_file = _wfopen(log_path.c_str(), L"w");
    if (!log_file) return 1;
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    HRESULT initialized = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(initialized)) { std::fclose(log_file); return 2; }
    WNDCLASSW klass{};
    klass.lpfnWndProc = events; klass.hInstance = instance;
    klass.lpszClassName = L"PulseEchoWebView2";
    klass.hCursor = LoadCursorW(nullptr, IDC_ARROW);
    RegisterClassW(&klass);
    RECT size{0, 0, 1000, 700};
    AdjustWindowRect(&size, WS_OVERLAPPEDWINDOW, FALSE);
    window = CreateWindowW(klass.lpszClassName, L"echo-loading", WS_OVERLAPPEDWINDOW,
                           CW_USEDEFAULT, CW_USEDEFAULT, size.right - size.left, size.bottom - size.top,
                           nullptr, nullptr, instance, nullptr);
    if (!window) { CoUninitialize(); std::fclose(log_file); return 2; }
    ShowWindow(window, show);
    wchar_t executable[32768];
    DWORD length = GetModuleFileNameW(nullptr, executable, 32768);
    if (!length || length >= 32768) { DestroyWindow(window); CoUninitialize(); std::fclose(log_file); return 2; }
    std::wstring loader_path(executable);
    loader_path.resize(loader_path.find_last_of(L"\\") + 1);
    loader_path += L"WebView2Loader.dll";
    HMODULE loader = LoadLibraryExW(loader_path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    using CreateEnvironment = HRESULT(STDAPICALLTYPE *)(PCWSTR, PCWSTR, ICoreWebView2EnvironmentOptions *, ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler *);
    auto create = loader ? reinterpret_cast<CreateEnvironment>(reinterpret_cast<void *>(GetProcAddress(loader, "CreateCoreWebView2EnvironmentWithOptions"))) : nullptr;
    if (!create) {
        std::fprintf(log_file, "WebView2Loader.dll unavailable: %lu\n", GetLastError());
        DestroyWindow(window); CoUninitialize(); std::fclose(log_file); return 2;
    }
    using EnvironmentReady = Handler<ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler, HRESULT, ICoreWebView2Environment *>;
    auto ready = new EnvironmentReady([](HRESULT result, ICoreWebView2Environment *env) -> HRESULT {
        if (FAILED(result) || !env) { check(FAILED(result) ? result : E_FAIL, "create_environment"); return S_OK; }
        environment = env; environment->AddRef();
        LPWSTR version = nullptr;
        if (SUCCEEDED(environment->get_BrowserVersionString(&version))) {
            std::fprintf(log_file, "runtime=%ls\n", version); std::fflush(log_file); CoTaskMemFree(version);
        }
        using ControllerReady = Handler<ICoreWebView2CreateCoreWebView2ControllerCompletedHandler, HRESULT, ICoreWebView2Controller *>;
        auto created = new ControllerReady([](HRESULT status, ICoreWebView2Controller *ctrl) -> HRESULT {
            if (FAILED(status) || !ctrl) { check(FAILED(status) ? status : E_FAIL, "create_controller"); return S_OK; }
            controller = ctrl; controller->AddRef();
            check(controller->get_CoreWebView2(&webview), "get_webview");
            if (!webview) return S_OK;
            RECT bounds; GetClientRect(window, &bounds);
            check(controller->put_Bounds(bounds), "bounds");
            using TitleChanged = Handler<ICoreWebView2DocumentTitleChangedEventHandler, ICoreWebView2 *, IUnknown *>;
            auto title = new TitleChanged([](ICoreWebView2 *sender, IUnknown *) -> HRESULT {
                LPWSTR text = nullptr;
                if (SUCCEEDED(sender->get_DocumentTitle(&text))) { SetWindowTextW(window, text); CoTaskMemFree(text); }
                return S_OK;
            });
            EventRegistrationToken token{};
            check(webview->add_DocumentTitleChanged(title, &token), "title_handler");
            title->Release();
            using NavigationReady = Handler<ICoreWebView2NavigationCompletedEventHandler, ICoreWebView2 *, ICoreWebView2NavigationCompletedEventArgs *>;
            auto navigated = new NavigationReady([](ICoreWebView2 *, ICoreWebView2NavigationCompletedEventArgs *args) -> HRESULT {
                BOOL success = FALSE;
                args->get_IsSuccess(&success);
                if (!success) { check(E_FAIL, "navigate"); return S_OK; }
                SetForegroundWindow(window);
                check(controller->MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC), "focus");
                return S_OK;
            });
            check(webview->add_NavigationCompleted(navigated, &token), "navigation_handler");
            navigated->Release();
            check(webview->Navigate(uri.c_str()), "navigate_start");
            return S_OK;
        });
        check(environment->CreateCoreWebView2Controller(window, created), "create_controller_start");
        created->Release();
        return S_OK;
    });
    check(create(runtime.empty() ? nullptr : runtime.c_str(), profile.c_str(), nullptr, ready), "create_environment_start");
    ready->Release();
    MSG message{};
    while (GetMessageW(&message, nullptr, 0, 0) > 0) { TranslateMessage(&message); DispatchMessageW(&message); }
    if (webview) webview->Release();
    if (controller) controller->Release();
    if (environment) environment->Release();
    CoUninitialize();
    FreeLibrary(loader);
    std::fclose(log_file);
    return exit_code;
}
