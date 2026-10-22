// Fixed Linux/X11 presentation fixtures; no shell, remote page or product authority.
#define _POSIX_C_SOURCE 200809L
#include <X11/Xlib.h>
#include <X11/keysym.h>
#include <gtk/gtk.h>
#include <gdk/gdkx.h>
#include <glib-unix.h>
#include <webkit2/webkit2.h>
#include <stdio.h>
#include <string.h>

static int exit_code;
static gboolean stop(gpointer unused) {
    (void)unused;
    gtk_main_quit();
    return G_SOURCE_REMOVE;
}
static void failed(WebKitWebView *view, WebKitWebProcessTerminationReason reason, gpointer unused) {
    (void)view; (void)unused;
    fprintf(stderr, "fixture.web_process_terminated:%d\n", reason);
    exit_code = 81;
    gtk_main_quit();
}
static const char *acceleration_name(WebKitHardwareAccelerationPolicy policy) {
    switch (policy) {
        case WEBKIT_HARDWARE_ACCELERATION_POLICY_ON_DEMAND: return "on-demand";
        case WEBKIT_HARDWARE_ACCELERATION_POLICY_ALWAYS: return "always";
        case WEBKIT_HARDWARE_ACCELERATION_POLICY_NEVER: return "never";
        default: return "unknown";
    }
}
static void ready(gpointer window) {
    WebKitWebView *view = WEBKIT_WEB_VIEW(gtk_bin_get_child(GTK_BIN(window)));
    printf("{\"ready\":true,\"xid\":%lu,\"webkit\":\"%u.%u.%u\",\"gtk\":\"%u.%u.%u\",\"requestedAccelerationPolicy\":\"%s\",\"actualAccelerationPolicy\":\"%s\"}\n",
        (unsigned long)GDK_WINDOW_XID(gtk_widget_get_window(GTK_WIDGET(window))),
        webkit_get_major_version(), webkit_get_minor_version(), webkit_get_micro_version(),
        gtk_get_major_version(), gtk_get_minor_version(), gtk_get_micro_version(),
        (const char *)g_object_get_data(G_OBJECT(window), "fixture-acceleration"),
        acceleration_name(webkit_settings_get_hardware_acceleration_policy(webkit_web_view_get_settings(view))));
    fflush(stdout);
}
static void loaded(WebKitWebView *view, WebKitLoadEvent event, gpointer window) {
    if (event != WEBKIT_LOAD_FINISHED || g_object_get_data(G_OBJECT(view), "fixture-xterm")) return;
    gtk_widget_grab_focus(GTK_WIDGET(view));
    ready(window);
}
static void xterm_ready(WebKitUserContentManager *manager, gpointer result, gpointer window) {
    (void)manager; (void)result;
    gtk_widget_grab_focus(gtk_bin_get_child(GTK_BIN(window)));
    ready(window);
}
static int native(void) {
    Display *display = XOpenDisplay(NULL);
    if (!display) return 73;
    int screen = DefaultScreen(display);
    Window window = XCreateSimpleWindow(display, RootWindow(display, screen), 0, 0, 900, 650, 0,
        BlackPixel(display, screen), BlackPixel(display, screen));
    XStoreName(display, window, "PulseTerm native presentation fixture");
    XSelectInput(display, window, KeyPressMask | KeyReleaseMask | ExposureMask | StructureNotifyMask);
    XMapWindow(display, window);
    GC gc = XCreateGC(display, window, 0, NULL);
    int ready = 0;
    for (;;) {
        XEvent event;
        XNextEvent(display, &event);
        if (event.type == MapNotify && !ready) {
            ready = 1;
            printf("{\"ready\":true,\"xid\":%lu,\"renderer\":\"XFillRectangle\"}\n", window);
            fflush(stdout);
        }
        if (event.type == KeyRelease && XLookupKeysym(&event.xkey, 0) == XK_q) break;
        if (event.type == KeyPress) {
            KeySym key = XLookupKeysym(&event.xkey, 0);
            if (key == XK_q) continue;
            if (key != XK_o && key != XK_x) continue;
            XSetForeground(display, gc, key == XK_o ? WhitePixel(display, screen) : BlackPixel(display, screen));
            XFillRectangle(display, window, gc, 50, 100, 8, 20);
            XFlush(display);
        }
    }
    XFreeGC(display, gc);
    XDestroyWindow(display, window);
    XCloseDisplay(display);
    return 0;
}
int main(int argc, char **argv) {
    if (argc != 2 && argc != 4) return 71;
    if (!strcmp(argv[1], "native")) return native();
    int animation = !strcmp(argv[1], "webkit-raf");
    int xterm = !strcmp(argv[1], "xterm-dirty") || !strcmp(argv[1], "xterm-full");
    if ((!animation && !xterm && strcmp(argv[1], "webkit-immediate")) || (xterm && argc != 4)) return 72;
    if (!gtk_init_check(NULL, NULL)) return 73;
    GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_decorated(GTK_WINDOW(window), FALSE);
    gtk_window_set_default_size(GTK_WINDOW(window), 900, 650);
    gtk_window_set_title(GTK_WINDOW(window), "PulseTerm WebKit presentation fixture");
    const char *requested_policy = g_getenv("PULSE_PRESENTATION_ACCELERATION");
    if (!requested_policy) requested_policy = "default";
    if (strcmp(requested_policy, "default") && strcmp(requested_policy, "never")) return 76;
    WebKitUserContentManager *manager = webkit_user_content_manager_new();
    if (xterm) {
        char *bundle = NULL;
        gsize length = 0;
        if (!g_file_get_contents(argv[2], &bundle, &length, NULL) || length > 2 * 1024 * 1024) return 74;
        WebKitUserScript *script = webkit_user_script_new(bundle, WEBKIT_USER_CONTENT_INJECT_TOP_FRAME,
            WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_START, NULL, NULL);
        webkit_user_content_manager_add_script(manager, script);
        webkit_user_script_unref(script);
        g_free(bundle);
        webkit_user_content_manager_register_script_message_handler(manager, "ready");
        g_signal_connect(manager, "script-message-received::ready", G_CALLBACK(xterm_ready), window);
    }
    WebKitSettings *settings = webkit_settings_new();
    if (!strcmp(requested_policy, "never"))
        webkit_settings_set_hardware_acceleration_policy(settings, WEBKIT_HARDWARE_ACCELERATION_POLICY_NEVER);
    WebKitWebView *view = WEBKIT_WEB_VIEW(g_object_new(WEBKIT_TYPE_WEB_VIEW,
        "user-content-manager", manager, "settings", settings, NULL));
    g_object_unref(settings);
    g_object_set_data(G_OBJECT(window), "fixture-acceleration", (gpointer)requested_policy);
    g_object_unref(manager);
    gtk_container_add(GTK_CONTAINER(window), GTK_WIDGET(view));
    g_signal_connect(view, "load-changed", G_CALLBACK(loaded), window);
    g_object_set_data(G_OBJECT(view), "fixture-xterm", GINT_TO_POINTER(xterm));
    g_signal_connect(view, "web-process-terminated", G_CALLBACK(failed), NULL);
    g_unix_signal_add(SIGTERM, stop, NULL);
    char *html = g_strdup_printf(
        "<!doctype html><meta charset='utf-8'><meta http-equiv='Content-Security-Policy' "
        "content=\"default-src 'none'; script-src 'nonce-fixture'; style-src 'nonce-fixture'\">"
        "<style nonce='fixture'>html,body{margin:0;background:black}#pixel{position:absolute;left:50px;top:100px;width:8px;height:20px;background:black}</style>"
        "<div id='pixel'></div><script nonce='fixture'>"
        "const pixel=document.getElementById('pixel');addEventListener('keydown',event=>{"
        "if(!event.isTrusted||event.repeat||!['o','x'].includes(event.key))return;"
        "const draw=()=>{pixel.style.backgroundColor=event.key==='o'?'white':'black'};"
        "%s});</script>", animation ? "requestAnimationFrame(draw)" : "draw()");
    if (xterm) {
        char *css = NULL;
        gsize length = 0;
        if (!g_file_get_contents(argv[3], &css, &length, NULL) || length > 128 * 1024) return 75;
        g_free(html);
        html = g_strdup_printf(
            "<!doctype html><meta charset='utf-8'><meta http-equiv='Content-Security-Policy' "
            "content=\"default-src 'none'; script-src 'nonce-fixture'; style-src 'nonce-fixture'\">"
            "<style nonce='fixture'>%s html,body{margin:0;background:black}#terminal{position:absolute;left:50px;top:100px;width:840px;height:500px}</style>"
            "<div id='terminal'></div><script nonce='fixture'>"
            "const documentOverride=new Proxy(document,{get(target,key){if(key==='createElement')return(tag,options)=>{"
            "const node=target.createElement(tag,options);if(tag.toLowerCase()==='style')node.setAttribute('nonce','fixture');return node};"
            "const value=Reflect.get(target,key,target);return typeof value==='function'?value.bind(target):value}});"
            "const terminal=new Terminal({documentOverride,cols:100,rows:30,fontSize:14,fontFamily:'monospace',"
            "cursorBlink:false,scrollback:1000,allowProposedApi:true,theme:{background:'#000000',foreground:'#ffffff'}});"
            "terminal.open(document.getElementById('terminal'));"
            "terminal.onData(key=>{if(key!=='o'&&key!=='x')return;terminal.write('\\x1b[1;1H'+(key==='o'?'O':'X'),()=>{%s})});"
            "terminal.write('X',()=>{terminal.focus();window.webkit.messageHandlers.ready.postMessage(true)});</script>",
            css, !strcmp(argv[1], "xterm-full") ? "terminal.refresh(0,terminal.rows-1)" : "");
        g_free(css);
    }
    webkit_web_view_load_html(view, html, "about:blank");
    g_free(html);
    gtk_widget_show_all(window);
    gtk_main();
    gtk_widget_destroy(window);
    return exit_code;
}
