/* Task-scoped AT-SPI observer and UI actuator for one explicitly selected application. */
#include <atspi/atspi.h>
#include <errno.h>
#include <string.h>
#include <stdio.h>

static unsigned remaining = 1024;
static gint64 deadline;
static unsigned selected_applications;

static gboolean action_mode;
static gboolean focus_mode;
static gboolean bounds_mode;
static gboolean inspect_mode;
static gboolean tree_mode;
static gboolean combo_inspect_mode;
static gboolean checkbox_inspect_mode;
static gboolean combo_bounds_mode;
static gboolean description_mode;
static gboolean readonly_text_mode;
static gint selection_index = -1;
static gboolean select_visible(AtspiAccessible *node, unsigned depth) {
    if (depth > 8 || remaining == 0 || g_get_monotonic_time() >= deadline) return FALSE;
    --remaining;
    GError *error = NULL;
    AtspiSelection *selection = atspi_accessible_get_selection_iface(node);
    if (selection) {
        gboolean changed = atspi_selection_select_child(selection, selection_index, &error);
        g_clear_error(&error);
        AtspiAccessible *chosen = changed ? atspi_selection_get_selected_child(selection, 0, &error) : NULL;
        char *name = chosen && !error ? atspi_accessible_get_name(chosen, &error) : NULL;
        gboolean confirmed = changed && chosen && name && !error;
        if (confirmed) printf("selected=%s\n", name);
        g_free(name);
        if (chosen) g_object_unref(chosen);
        g_clear_error(&error);
        g_object_unref(selection);
        if (confirmed) return TRUE;
    }
    int count = atspi_accessible_get_child_count(node, &error);
    g_clear_error(&error);
    for (int index = 0; index < count && index < 768; ++index) {
        AtspiAccessible *child = atspi_accessible_get_child_at_index(node, index, &error);
        gboolean selected = child && !error && select_visible(child, depth + 1);
        if (child) g_object_unref(child);
        g_clear_error(&error);
        if (selected) return TRUE;
    }
    return FALSE;
}
static gboolean activate_match(AtspiAccessible *node) {
    if (combo_bounds_mode && atspi_accessible_get_role(node, NULL) != ATSPI_ROLE_COMBO_BOX) return FALSE;
    if (checkbox_inspect_mode && atspi_accessible_get_role(node, NULL) != ATSPI_ROLE_CHECK_BOX) return FALSE;
    if (readonly_text_mode) {
        GError *error = NULL;
        AtspiStateSet *states = atspi_accessible_get_state_set(node);
        gboolean editable = !states || atspi_state_set_contains(states, ATSPI_STATE_EDITABLE);
        if (states) g_object_unref(states);
        AtspiRole role = atspi_accessible_get_role(node, &error);
        if (editable || error || role == ATSPI_ROLE_PASSWORD_TEXT) { g_clear_error(&error); return FALSE; }
        AtspiText *text = atspi_accessible_get_text_iface(node);
        if (!text) return FALSE;
        int length = atspi_text_get_character_count(text, &error);
        char *value = !error && length > 0 && length <= 4096 ? atspi_text_get_text(text, 0, length, &error) : NULL;
        gboolean success = value && !error && strlen(value) <= 16384;
        if (success) printf("%s\n", value);
        g_free(value); g_clear_error(&error); g_object_unref(text);
        return success;
    }
    if (description_mode) {
        GError *error = NULL;
        char *description = atspi_accessible_get_description(node, &error);
        gboolean success = description && *description && strlen(description) <= 512 && !error;
        if (success) printf("%s\n", description);
        g_free(description);
        g_clear_error(&error);
        return success;
    }
    if (selection_index >= 0) return select_visible(node, 0);
    if (inspect_mode) {
        if (combo_inspect_mode && atspi_accessible_get_role(node, NULL) != ATSPI_ROLE_COMBO_BOX) return FALSE;
        AtspiStateSet *states = atspi_accessible_get_state_set(node);
        printf("showing=%d enabled=%d focusable=%d selected=%d checked=%d expanded=%d\n", states && atspi_state_set_contains(states, ATSPI_STATE_SHOWING), states && atspi_state_set_contains(states, ATSPI_STATE_ENABLED), states && atspi_state_set_contains(states, ATSPI_STATE_FOCUSABLE), states && atspi_state_set_contains(states, ATSPI_STATE_SELECTED), states && atspi_state_set_contains(states, ATSPI_STATE_CHECKED), states && atspi_state_set_contains(states, ATSPI_STATE_EXPANDED));
        if (states) g_object_unref(states);
        return TRUE;
    }
    if (bounds_mode) {
        GError *error = NULL;
        AtspiComponent *component = atspi_accessible_get_component_iface(node);
        if (!component) return FALSE;
        AtspiRect *rect = atspi_component_get_extents(component, ATSPI_COORD_TYPE_SCREEN, &error);
        gboolean success = !error && rect && rect->width > 0 && rect->height > 0;
        if (success) printf("%d %d %d %d\n", rect->x, rect->y, rect->width, rect->height);
        g_clear_error(&error);
        g_free(rect);
        g_object_unref(component);
        return success;
    }
    if (!action_mode && !focus_mode) return TRUE;
    AtspiStateSet *states = atspi_accessible_get_state_set(node);
    gboolean enabled = states && atspi_state_set_contains(states, ATSPI_STATE_ENABLED);
    gboolean editable = states && atspi_state_set_contains(states, ATSPI_STATE_EDITABLE);
    gboolean focusable = states && atspi_state_set_contains(states, ATSPI_STATE_FOCUSABLE);
    if (states) g_object_unref(states);
    if (focus_mode) {
        if (!enabled || (!editable && !focusable)) return FALSE;
        AtspiComponent *component = atspi_accessible_get_component_iface(node);
        if (!component) return FALSE;
        GError *error = NULL;
        gboolean applied = atspi_component_grab_focus(component, &error);
        gboolean success = applied && !error;
        g_clear_error(&error);
        g_object_unref(component);
        return success;
    }
    if (!enabled) return FALSE;
    AtspiAction *action = atspi_accessible_get_action_iface(node);
    if (!action) return FALSE;
    GError *error = NULL;
    int count = atspi_action_get_n_actions(action, &error);
    gboolean applied = !error && count > 0 && atspi_action_do_action(action, 0, &error);
    gboolean success = applied && !error;
    g_clear_error(&error);
    g_object_unref(action);
    return success;
}
static gboolean matches(AtspiAccessible *node, const char *expected, unsigned depth) {
    if (depth > 32 || remaining == 0 || g_get_monotonic_time() >= deadline) return FALSE;
    --remaining;
    GError *error = NULL;
    AtspiStateSet *states = atspi_accessible_get_state_set(node);
    gboolean showing = states && atspi_state_set_contains(states, ATSPI_STATE_SHOWING);
    gboolean sensitive = !states || atspi_state_set_contains(states, ATSPI_STATE_EDITABLE);
    if (tree_mode) {
        AtspiRole role = atspi_accessible_get_role(node, &error);
        sensitive = sensitive || error || role == ATSPI_ROLE_PASSWORD_TEXT;
        fprintf(stderr, "node=%u depth=%u role=%d showing=%d sensitive=%d role_error=%d\n", 1024 - remaining, depth, role, showing, sensitive, error != NULL);
        g_clear_error(&error);
    }
    if (states) g_object_unref(states);
    if (showing || inspect_mode) {
        char *name = atspi_accessible_get_name(node, &error);
        gboolean found = !error && name && strcmp(name, expected) == 0;
        /* Structure only: never emit names, text values or editable-field lengths. */
        if (tree_mode) fprintf(stderr, "name_bytes=%ld name_match=%d name_error=%d\n", !sensitive && name ? (long)strlen(name) : -1L, found, error != NULL);
        g_clear_error(&error);
        g_free(name);
        if (found && activate_match(node)) return TRUE;
        AtspiText *text = atspi_accessible_get_text_iface(node);
        if (text) {
            int length = atspi_text_get_character_count(text, &error);
            char *value = !error && length > 0 && length <= 512 ? atspi_text_get_text(text, 0, length, &error) : NULL;
            found = !error && value && strcmp(value, expected) == 0;
            if (tree_mode) fprintf(stderr, "text_chars=%d text_match=%d text_error=%d\n", sensitive ? -1 : length, found, error != NULL);
            g_clear_error(&error);
            g_free(value);
            g_object_unref(text);
            if (found && activate_match(node)) return TRUE;
        }
    }
    int count = atspi_accessible_get_child_count(node, &error);
    if (tree_mode) fprintf(stderr, "children=%d children_error=%d\n", count, error != NULL);
    if (error || count < 0 || (unsigned)count > remaining) {
        g_clear_error(&error);
        return FALSE;
    }
    /* Search modal controls before unrelated background content within the same budget. */
    for (unsigned pass = 0; pass < 2; ++pass) {
        for (int index = 0; index < count; ++index) {
            if (g_get_monotonic_time() >= deadline) return FALSE;
            AtspiAccessible *child = atspi_accessible_get_child_at_index(node, index, &error);
            if (error || !child) {
                g_clear_error(&error);
                if (child) g_object_unref(child);
                continue;
            }
            AtspiRole role = atspi_accessible_get_role(child, &error);
            gboolean modal = !error && (role == ATSPI_ROLE_DIALOG || role == ATSPI_ROLE_ALERT);
            g_clear_error(&error);
            gboolean found = modal == (pass == 0) && matches(child, expected, depth + 1);
            g_object_unref(child);
            if (found) return TRUE;
        }
    }
    return FALSE;
}

/* Optional exact top-level frame scope for same-process multiwindow probes. */
static gboolean matches_frame(AtspiAccessible *node, const char *frame, const char *expected, unsigned depth) {
    if (depth > 32 || remaining == 0 || g_get_monotonic_time() >= deadline) return FALSE;
    --remaining;
    GError *error = NULL;
    AtspiRole role = atspi_accessible_get_role(node, &error);
    g_clear_error(&error);
    if (role == ATSPI_ROLE_FRAME) {
        char *name = atspi_accessible_get_name(node, &error);
        gboolean selected = !error && name && strcmp(name, frame) == 0;
        g_clear_error(&error);
        g_free(name);
        return selected && matches(node, expected, 0);
    }
    int count = atspi_accessible_get_child_count(node, &error);
    if (error || count < 0 || (unsigned)count > remaining) { g_clear_error(&error); return FALSE; }
    for (int index = 0; index < count; ++index) {
        AtspiAccessible *child = atspi_accessible_get_child_at_index(node, index, &error);
        gboolean found = child && !error && matches_frame(child, frame, expected, depth + 1);
        g_clear_error(&error);
        if (child) g_object_unref(child);
        if (found) return TRUE;
    }
    return FALSE;
}

int main(int argc, char **argv) {
    if ((argc != 3 && argc != 4) || !argv[1][0] || !argv[2][0] || (argc == 4 && strcmp(argv[3], "activate") != 0 && strcmp(argv[3], "focus") != 0 && strcmp(argv[3], "bounds") != 0 && strcmp(argv[3], "bounds-combo") != 0 && strcmp(argv[3], "inspect") != 0 && strcmp(argv[3], "inspect-tree") != 0 && strcmp(argv[3], "inspect-combo") != 0 && strcmp(argv[3], "inspect-checkbox") != 0 && strcmp(argv[3], "description") != 0 && strcmp(argv[3], "readonly-text") != 0 && strncmp(argv[3], "select=", 7) != 0)) return 2;
    action_mode = argc == 4 && strcmp(argv[3], "activate") == 0;
    focus_mode = argc == 4 && strcmp(argv[3], "focus") == 0;
    combo_bounds_mode = argc == 4 && strcmp(argv[3], "bounds-combo") == 0;
    bounds_mode = combo_bounds_mode || (argc == 4 && strcmp(argv[3], "bounds") == 0);
    combo_inspect_mode = argc == 4 && strcmp(argv[3], "inspect-combo") == 0;
    checkbox_inspect_mode = argc == 4 && strcmp(argv[3], "inspect-checkbox") == 0;
    tree_mode = argc == 4 && strcmp(argv[3], "inspect-tree") == 0;
    inspect_mode = tree_mode || combo_inspect_mode || checkbox_inspect_mode || (argc == 4 && strcmp(argv[3], "inspect") == 0);
    description_mode = argc == 4 && strcmp(argv[3], "description") == 0;
    readonly_text_mode = argc == 4 && strcmp(argv[3], "readonly-text") == 0;
    char *end = NULL;
    if (argc == 4 && strncmp(argv[3], "select=", 7) == 0) {
        errno = 0;
        gint64 option = g_ascii_strtoll(argv[3] + 7, &end, 10);
        if (errno || !end || end == argv[3] + 7 || *end || option < 0 || option > 768) return 2;
        selection_index = (gint)option;
    }
    errno = 0;
    guint64 pid = g_ascii_strtoull(argv[1], &end, 10);
    if (errno || !end || *end || pid == 0 || pid > G_MAXUINT) return 2;
    if (atspi_init() != 0) return 2;
    atspi_set_timeout(200, 400);
    deadline = g_get_monotonic_time() + G_USEC_PER_SEC;
    AtspiAccessible *desktop = atspi_get_desktop(0);
    gboolean found = FALSE;
    GError *error = NULL;
    if (desktop) {
        int count = atspi_accessible_get_child_count(desktop, &error);
        g_clear_error(&error);
        for (int index = 0; index < count && index < 128 && !found; ++index) {
            if (g_get_monotonic_time() >= deadline) break;
            AtspiAccessible *application = atspi_accessible_get_child_at_index(desktop, index, &error);
            if (!application || error) {
                g_clear_error(&error);
                if (application) g_object_unref(application);
                continue;
            }
            guint actual = atspi_accessible_get_process_id(application, &error);
            if (!error && actual == (guint)pid) { ++selected_applications; const char *frame = g_getenv("PULSETERM_PROBE_FRAME"); found = frame && *frame ? matches_frame(application, frame, argv[2], 0) : matches(application, argv[2], 0); }
            g_clear_error(&error);
            g_object_unref(application);
        }
        g_object_unref(desktop);
    }
    if (!found) fprintf(stderr, "selected_applications=%u visited_nodes=%u deadline_reached=%d node_limit_reached=%d\n", selected_applications, 1024 - remaining, g_get_monotonic_time() >= deadline, remaining == 0);
    atspi_exit();
    return found ? 0 : 1;
}
