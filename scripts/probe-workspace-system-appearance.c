/* Isolated desktop Settings portal for native system-appearance acceptance.
 * Start only inside the probe's private dbus-run-session; never replaces an owner.
 * Implements the legacy nested Read result consumed by locked Tao 0.37.1.
 */
#include <gio/gio.h>
#include <glib-unix.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>

static GMainLoop *loop;
static guint32 scheme = 2;
static int failed;
static const char *namespace = "org.freedesktop.appearance";
static const char *key = "color-scheme";
static const char *object_path = "/org/freedesktop/portal/desktop";
static const char *interface = "org.freedesktop.portal.Settings";
static const char xml[] =
    "<node><interface name='org.freedesktop.portal.Settings'>"
    "<method name='Read'><arg type='s' direction='in'/><arg type='s' direction='in'/>"
    "<arg type='v' direction='out'/></method>"
    "<method name='ReadAll'><arg type='as' direction='in'/>"
    "<arg type='a{sa{sv}}' direction='out'/></method>"
    "<signal name='SettingChanged'><arg type='s'/><arg type='s'/><arg type='v'/></signal>"
    "<property name='version' type='u' access='read'/></interface></node>";

static void method(GDBusConnection *connection, const gchar *sender,
                   const gchar *path, const gchar *iface, const gchar *name,
                   GVariant *args, GDBusMethodInvocation *call, gpointer data) {
    (void)connection; (void)sender; (void)path; (void)iface; (void)data;
    if (!strcmp(name, "Read")) {
        const gchar *wanted_namespace, *wanted_key;
        g_variant_get(args, "(&s&s)", &wanted_namespace, &wanted_key);
        if (strcmp(wanted_namespace, namespace) || strcmp(wanted_key, key)) {
            g_dbus_method_invocation_return_dbus_error(call,
                "org.freedesktop.portal.Error.NotFound", "Unknown fixture setting");
            return;
        }
        g_dbus_method_invocation_return_value(call,
            g_variant_new("(v)", g_variant_new_variant(g_variant_new_uint32(scheme))));
        printf("fixture.system_appearance.read.%u\n", scheme);
    } else if (!strcmp(name, "ReadAll")) {
        GVariantIter *patterns;
        const gchar *pattern;
        g_variant_get(args, "(as)", &patterns);
        gboolean include = g_variant_iter_n_children(patterns) == 0;
        while (g_variant_iter_next(patterns, "&s", &pattern))
            include |= g_pattern_match_simple(pattern, namespace);
        g_variant_iter_free(patterns);
        GVariantBuilder outer, inner;
        g_variant_builder_init(&outer, G_VARIANT_TYPE("a{sa{sv}}"));
        if (include) {
            g_variant_builder_init(&inner, G_VARIANT_TYPE("a{sv}"));
            g_variant_builder_add(&inner, "{sv}", key, g_variant_new_uint32(scheme));
            g_variant_builder_add(&outer, "{s@a{sv}}", namespace,
                                  g_variant_builder_end(&inner));
        }
        g_dbus_method_invocation_return_value(call, g_variant_new("(a{sa{sv}})", &outer));
    } else {
        g_dbus_method_invocation_return_dbus_error(call,
            "org.freedesktop.DBus.Error.UnknownMethod", "Unknown fixture method");
    }
}
static GVariant *property(GDBusConnection *connection, const gchar *sender,
                          const gchar *path, const gchar *iface,
                          const gchar *name, GError **error, gpointer data) {
    (void)connection; (void)sender; (void)path; (void)iface; (void)error; (void)data;
    return !strcmp(name, "version") ? g_variant_new_uint32(1) : NULL;
}
static gboolean stop(gpointer data) {
    (void)data;
    g_main_loop_quit(loop);
    return G_SOURCE_REMOVE;
}
static gboolean input(gint fd, GIOCondition condition, gpointer data) {
    (void)fd;
    GDBusConnection *connection = data;
    char command[32];
    if (!(condition & G_IO_IN) || !fgets(command, sizeof(command), stdin)) return stop(NULL);
    if (!strcmp(command, "quit\n")) return stop(NULL);
    if (!strcmp(command, "dark\n")) scheme = 1;
    else if (!strcmp(command, "light\n")) scheme = 2;
    else { failed = 1; return stop(NULL); }
    if (!g_dbus_connection_emit_signal(connection, NULL, object_path, interface,
            "SettingChanged", g_variant_new("(ssv)", namespace, key,
            g_variant_new_uint32(scheme)), NULL) ||
        !g_dbus_connection_flush_sync(connection, NULL, NULL)) {
        failed = 1;
        return stop(NULL);
    }
    printf("fixture.system_appearance.changed.%u\n", scheme);
    return G_SOURCE_CONTINUE;
}
int main(void) {
    if (g_strcmp0(g_getenv("PULSE_APPEARANCE_FIXTURE"), "1") ||
        !g_getenv("DBUS_SESSION_BUS_ADDRESS")) return 2;
    setvbuf(stdout, NULL, _IOLBF, 0);
    GError *error = NULL;
    GDBusNodeInfo *info = g_dbus_node_info_new_for_xml(xml, &error);
    GDBusConnection *connection = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, &error);
    if (!info || !connection) return 3;
    const GDBusInterfaceVTable vtable = {
        .method_call = method, .get_property = property
    };
    guint object = g_dbus_connection_register_object(connection, object_path,
        info->interfaces[0], &vtable, NULL, NULL, &error);
    if (!object) return 4;
    GVariant *reply = g_dbus_connection_call_sync(connection,
        "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
        "RequestName", g_variant_new("(su)", "org.freedesktop.portal.Desktop", 4u),
        G_VARIANT_TYPE("(u)"), G_DBUS_CALL_FLAGS_NONE, 3000, NULL, &error);
    guint32 owner = 0;
    if (reply) { g_variant_get(reply, "(u)", &owner); g_variant_unref(reply); }
    if (owner != 1) return 5;
    loop = g_main_loop_new(NULL, FALSE);
    guint reader = g_unix_fd_add(0, G_IO_IN | G_IO_HUP, input, connection);
    guint interrupt = g_unix_signal_add(SIGINT, stop, NULL);
    guint terminate = g_unix_signal_add(SIGTERM, stop, NULL);
    puts("fixture.system_appearance.ready");
    g_main_loop_run(loop);
    GSource *source = g_main_context_find_source_by_id(NULL, reader);
    if (source) g_source_destroy(source);
    source = g_main_context_find_source_by_id(NULL, interrupt);
    if (source) g_source_destroy(source);
    source = g_main_context_find_source_by_id(NULL, terminate);
    if (source) g_source_destroy(source);
    g_dbus_connection_unregister_object(connection, object);
    g_dbus_connection_close_sync(connection, NULL, NULL);
    g_object_unref(connection);
    g_dbus_node_info_unref(info);
    g_main_loop_unref(loop);
    return failed;
}
