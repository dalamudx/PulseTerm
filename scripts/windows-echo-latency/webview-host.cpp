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
#include "navigation.h"

static HWND window;
static ICoreWebView2Environment *environment;
static ICoreWebView2Controller *controller;
static ICoreWebView2 *webview;
static FILE *log_file;
static int exit_code;
static std::wstring uri;
static EchoNavigation navigation;
static bool diagnostic;

#ifdef __MINGW32__
// The SDK's MIDL uuid attributes are MSVC-only; bind the same published interface IDs.
__CRT_UUID_DECL(ICoreWebView2DocumentTitleChangedEventHandler, 0xf5f2b923, 0x953e, 0x4042, 0x9f,0x95,0xf3,0xa1,0x18,0xe1,0xaf,0xd4)
__CRT_UUID_DECL(ICoreWebView2NavigationCompletedEventHandler, 0xd33a35bf, 0x1c49, 0x4f98, 0x93,0xab,0x00,0x6e,0x05,0x33,0xfe,0x1c)
__CRT_UUID_DECL(ICoreWebView2CreateCoreWebView2ControllerCompletedHandler, 0x6c4819f3, 0xc9b7, 0x4260, 0x81,0x27,0xc9,0xf5,0xbd,0xe7,0xf6,0x8c)
__CRT_UUID_DECL(ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler, 0x4e8a3389, 0xc9d8, 0x4bd2, 0xb6,0xb5,0x12,0x4f,0xee,0x6c,0xc1,0x4d)
__CRT_UUID_DECL(ICoreWebView2WebMessageReceivedEventHandler, 0x57213f19, 0x00e6, 0x49fa, 0x8e,0x07,0x89,0x8e,0xa0,0x1e,0xcb,0xd2)
__CRT_UUID_DECL(ICoreWebView2NavigationStartingEventHandler, 0x9adbe429, 0xf36d, 0x432b, 0x9d,0xdc,0xf8,0x88,0x1f,0xbd,0x76,0xe3)
__CRT_UUID_DECL(ICoreWebView2ExecuteScriptCompletedHandler, 0x49511172, 0xcc67, 0x4bca, 0x99,0x23,0x13,0x71,0x12,0xf4,0xc4,0xcc)
__CRT_UUID_DECL(ICoreWebView2ProcessFailedEventHandler, 0x79e0aea4, 0x990b, 0x42d9, 0xaa,0x1d,0x0f,0x3b,0xc3,0x57,0x14,0x98)
#endif

static_assert(COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_ABORTED == 9, "SDK connection-aborted value");
static_assert(COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED == 14, "SDK operation-canceled value");
static const char *web_error_name(COREWEBVIEW2_WEB_ERROR_STATUS error) {
    switch (error) {
    case COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN: return "UNKNOWN";
    case COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_ABORTED: return "CONNECTION_ABORTED";
    case COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_RESET: return "CONNECTION_RESET";
    case COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED: return "OPERATION_CANCELED";
    default: return "OTHER";
    }
}
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
        else if (flag == L"--diagnostic" && std::wstring(argv[i + 1]) == L"1") diagnostic = true;
        else { LocalFree(argv); return 1; }
    }
    LocalFree(argv);
    if (uri.rfind(L"file:///", 0) != 0 || profile.empty() || log_path.empty()) return 1;
    log_file = _wfopen(log_path.c_str(), L"w");
    if (!log_file) return 1;
    navigation.select(uri);
    std::fprintf(log_file, "navigation_request uri=%ls\n", uri.c_str()); std::fflush(log_file);
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
            using WebMessage = Handler<ICoreWebView2WebMessageReceivedEventHandler, ICoreWebView2 *, ICoreWebView2WebMessageReceivedEventArgs *>;
            auto message = new WebMessage([](ICoreWebView2 *, ICoreWebView2WebMessageReceivedEventArgs *args) -> HRESULT {
                static unsigned messages = 0;
                LPWSTR source = nullptr, text = nullptr;
                if (SUCCEEDED(args->get_Source(&source)) && source && echo_same_document(uri, source) &&
                    SUCCEEDED(args->TryGetWebMessageAsString(&text)) && text &&
                    wcslen(text) < 160 && wcsncmp(text, L"echo-event ", 11) == 0 && messages < 4096) {
                    LARGE_INTEGER tick, hz; QueryPerformanceCounter(&tick); QueryPerformanceFrequency(&hz);
                    std::fprintf(log_file, "qpc=%lld hz=%lld %ls\n", tick.QuadPart, hz.QuadPart, text);
                    std::fflush(log_file); messages++;
                }
                CoTaskMemFree(source); CoTaskMemFree(text);
                return S_OK;
            });
            check(webview->add_WebMessageReceived(message, &token), "message_handler");
            message->Release();
            using ProcessFailed = Handler<ICoreWebView2ProcessFailedEventHandler, ICoreWebView2 *, ICoreWebView2ProcessFailedEventArgs *>;
            auto failed = new ProcessFailed([](ICoreWebView2 *, ICoreWebView2ProcessFailedEventArgs *args) -> HRESULT {
                COREWEBVIEW2_PROCESS_FAILED_KIND kind = COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED;
                HRESULT result = args->get_ProcessFailedKind(&kind);
                std::fprintf(log_file, "process_failed kind=%d result=0x%08lx\n", static_cast<int>(kind), static_cast<unsigned long>(result));
                std::fflush(log_file);
                check(FAILED(result) ? result : E_FAIL, "process_failed");
                return S_OK;
            });
            check(webview->add_ProcessFailed(failed, &token), "process_failed_handler");
            failed->Release();
            using NavigationStarting = Handler<ICoreWebView2NavigationStartingEventHandler, ICoreWebView2 *, ICoreWebView2NavigationStartingEventArgs *>;
            auto starting = new NavigationStarting([](ICoreWebView2 *, ICoreWebView2NavigationStartingEventArgs *args) -> HRESULT {
                LPWSTR address = nullptr; UINT64 id = 0;
                HRESULT address_result = args->get_Uri(&address);
                HRESULT id_result = args->get_NavigationId(&id);
                if (SUCCEEDED(address_result) && SUCCEEDED(id_result) && address) {
                    bool target = navigation.begin(address, id);
                    std::fprintf(log_file, "navigation_start id=%llu target=%d uri=%ls\n", id, target ? 1 : 0, address);
                    std::fflush(log_file);
                } else {check(FAILED(address_result) ? address_result : id_result, "navigation_start_properties");}
                CoTaskMemFree(address);
                return S_OK;
            });
            check(webview->add_NavigationStarting(starting, &token), "navigation_start_handler");
            starting->Release();
            using NavigationReady = Handler<ICoreWebView2NavigationCompletedEventHandler, ICoreWebView2 *, ICoreWebView2NavigationCompletedEventArgs *>;
            auto navigated = new NavigationReady([](ICoreWebView2 *, ICoreWebView2NavigationCompletedEventArgs *args) -> HRESULT {
                BOOL success = FALSE; UINT64 id = 0;
                COREWEBVIEW2_WEB_ERROR_STATUS error = COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN;
                HRESULT id_result = args->get_NavigationId(&id);
                HRESULT success_result = args->get_IsSuccess(&success);
                HRESULT error_result = args->get_WebErrorStatus(&error);
                bool target = SUCCEEDED(id_result) && navigation.completes(id);
                std::fprintf(log_file, "navigation_complete id=%llu target=%d success=%d webErrorStatus=%d errorName=%s errorResult=0x%08lx\n",
                    id, target ? 1 : 0, success, static_cast<int>(error), web_error_name(error), static_cast<unsigned long>(error_result));
                std::fflush(log_file);
                if (FAILED(id_result) || FAILED(success_result)) {check(FAILED(id_result) ? id_result : success_result, "navigation_complete_properties");return S_OK;}
                if (!target) return S_OK;
                if (!success) {check(E_FAIL, "navigate");return S_OK;}
                SetForegroundWindow(window);
                check(controller->MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC), "focus");
                if (diagnostic) {
                    using ScriptComplete = Handler<ICoreWebView2ExecuteScriptCompletedHandler, HRESULT, LPCWSTR>;
                    auto completed = new ScriptComplete([](HRESULT result, LPCWSTR value) -> HRESULT {
                        bool enabled = SUCCEEDED(result) && value && wcscmp(value, L"true") == 0;
                        std::fprintf(log_file, "diagnostic_started success=%d result=0x%08lx\n", enabled ? 1 : 0, static_cast<unsigned long>(result));
                        std::fflush(log_file);
                        if (!enabled) check(FAILED(result) ? result : E_FAIL, "diagnostic_script");
                        return S_OK;
                    });
                    check(webview->ExecuteScript(L"window.enableEchoDiagnostics()", completed), "diagnostic_script_start");
                    completed->Release();
                }
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
