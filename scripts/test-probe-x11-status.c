/* Real GTK/D-Bus fixture with a deliberately stale client interface cache. */
#include <gtk/gtk.h>
#include <atspi/atspi.h>

static AtspiRole stale_role(AtspiAccessible *node, GError **error) {
    AtspiRole role = atspi_accessible_get_role(node, error);
    if ((!error || !*error) && role == ATSPI_ROLE_STATUS_BAR &&
        !g_object_get_data(G_OBJECT(node), "stale-injected")) {
        node->interfaces = 0;
        node->cached_properties |= ATSPI_CACHE_INTERFACES;
        g_object_set_data(G_OBJECT(node), "stale-injected", GINT_TO_POINTER(1));
    }
    return role;
}
static void refresh_cache(AtspiAccessible *node) {
    if (!g_getenv("PULSE_TEST_STALE_CACHE")) atspi_accessible_clear_cache_single(node);
}
#define atspi_accessible_get_role stale_role
#define atspi_accessible_clear_cache_single refresh_cache
#define main probe_main
#include "probe-x11-status.c"
#undef main
#undef atspi_accessible_clear_cache_single
#undef atspi_accessible_get_role

int main(int argc, char **argv) {
    if (argc != 2 || strcmp(argv[1], "--fixture") != 0) return probe_main(argc, argv);
    gtk_init(&argc, &argv);
    GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(window), "PulseTerm status cache fixture");
    GtkWidget *box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 0);
    gtk_container_add(GTK_CONTAINER(window), box);
    GtkWidget *label = gtk_label_new("Public refusal sentinel");
    AtkObject *accessible = gtk_widget_get_accessible(label);
    atk_object_set_role(accessible, ATK_ROLE_STATUSBAR);
    atk_object_set_name(accessible, "");
    gtk_box_pack_start(GTK_BOX(box), label, FALSE, FALSE, 0);
    GtkWidget *secret = gtk_entry_new();
    gtk_entry_set_visibility(GTK_ENTRY(secret), FALSE);
    gtk_entry_set_text(GTK_ENTRY(secret), "DO_NOT_LOG_SECRET_CONTENT");
    gtk_box_pack_start(GTK_BOX(box), secret, FALSE, FALSE, 0);
    gtk_widget_show_all(window);
    gtk_main();
    return 0;
}
