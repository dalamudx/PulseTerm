/* Test-only clipboard selection owner for the private Xvfb, no runtime dependency.
 * Uses GTK already required by the native Linux desktop; never user clipboard. */
#include <gtk/gtk.h>
#include <stdio.h>
#include <string.h>
static const gchar *release_gate = NULL;
static void clipboard_get(GtkClipboard *clipboard, GtkSelectionData *selection, guint info, gpointer data) {
  (void)clipboard; (void)info;
  puts("clipboard.requested"); fflush(stdout);
  if (release_gate) {
    gint64 deadline = g_get_monotonic_time() + 4000000;
    while (!g_file_test(release_gate, G_FILE_TEST_EXISTS) && g_get_monotonic_time() < deadline) g_usleep(10000);
    if (!g_file_test(release_gate, G_FILE_TEST_EXISTS)) {
      puts("clipboard.gate_timeout"); fflush(stdout); return;
    }
  }
  gtk_selection_data_set_text(selection, (const gchar *)data, -1);
  puts("clipboard.served"); fflush(stdout);
}
static void clipboard_clear(GtkClipboard *clipboard, gpointer data) { (void)clipboard; g_free(data); }
int main(int argc, char **argv) {
  if (!gtk_init_check(&argc, &argv)) return 2;
  if (argc == 3 && strcmp(argv[1], "read") == 0) {
    gchar *copied = gtk_clipboard_wait_for_text(gtk_clipboard_get(GDK_SELECTION_CLIPBOARD));
    if (!copied || strlen(copied) > 4194304) { g_free(copied); return 2; }
    gboolean ok = g_file_set_contents(argv[2], copied, -1, NULL);
    g_free(copied);
    return ok ? 0 : 2;
  }
  if (argc != 2 && argc != 3) return 2;
  if (argc == 3) {
    if (!g_path_is_absolute(argv[2])) return 2;
    release_gate = argv[2];
  }
  gchar *text = NULL;
  gsize length = 0;
  GError *error = NULL;
  if (!g_file_get_contents(argv[1], &text, &length, &error) || length > 1024 || !g_utf8_validate(text, (gssize)length, NULL)) {
    if (error) g_error_free(error);
    g_free(text);
    return 2;
  }
  GtkClipboard *clipboard = gtk_clipboard_get(GDK_SELECTION_CLIPBOARD);
  GtkTargetEntry targets[] = {{"UTF8_STRING", 0, 0}, {"TEXT", 0, 0}, {"text/plain", 0, 0}};
  if (!gtk_clipboard_set_with_data(clipboard, targets, 3, clipboard_get, clipboard_clear, text)) { g_free(text); return 2; }
  puts("clipboard.ready");
  fflush(stdout);
  gtk_main();
  return 0;
}
