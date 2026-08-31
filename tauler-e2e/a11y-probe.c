// Tiny AT-SPI client that reads tauler's a11y tree and can activate a node.
//
// The point of the whole e2e flow is to prove a real AT can see tauler's tree
// and drive a click through it. This is that AT — the smallest thing that can
// talk to the accessibility bus: enumerate the tree, and optionally perform
// the default action on a node whose accessible name matches.
//
//   a11y-probe                 print the whole desktop tree and exit 0
//   a11y-probe --activate NAME wait until a node named NAME has an action,
//                              perform it, print the tree, and exit 0
//
// The retry loop exists because tauler's accesskit adapter attaches lazily:
// it only registers its tree once an AT is on the bus (ADR 0039), which takes a
// moment after this probe connects. So the probe re-enumerates until it sees
// what it is after, instead of assuming the tree is there on the first try.

#include <atspi/atspi.h>
#include <glib.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static const char *activate_name = NULL;
static int activated = 0;
static int saw_nodes = 0;

static void walk(AtspiAccessible *node, int depth) {
    if (node == NULL) {
        return;
    }
    GError *err = NULL;
    gchar *name = atspi_accessible_get_name(node, &err);
    AtspiRole role = atspi_accessible_get_role(node, &err);
    const char *role_name = atspi_role_get_name(role);
    for (int i = 0; i < depth; i++) {
        printf("  ");
    }
    printf("%s\t%s\n", name ? name : "", role_name ? role_name : "");
    fflush(stdout);

    if (name && name[0] != '\0') {
        saw_nodes = 1;
    }

    if (!activated && activate_name && name && strcmp(name, activate_name) == 0) {
        AtspiAction *action = atspi_accessible_get_action_iface(node);
        if (action != NULL) {
            gint n = atspi_action_get_n_actions(action, NULL);
            if (n > 0) {
                if (atspi_action_do(action, 0, NULL)) {
                    printf("ACTIVATED:%s\n", name);
                    fflush(stdout);
                    activated = 1;
                }
            }
        }
    }

    gint n_children = atspi_accessible_get_child_count(node, &err);
    for (gint i = 0; i < n_children; i++) {
        AtspiAccessible *child = atspi_accessible_get_child_at_index(node, i, NULL);
        walk(child, depth + 1);
        if (child != NULL) {
            g_object_unref(child);
        }
    }
    if (name != NULL) {
        g_free(name);
    }
}

int main(int argc, char **argv) {
    if (argc >= 3 && strcmp(argv[1], "--activate") == 0) {
        activate_name = argv[2];
    }

    if (!atspi_init()) {
        fprintf(stderr, "a11y-probe: atspi_init failed (is the accessibility bus up?)\n");
        return 1;
    }

    for (int attempt = 0; attempt < 50; attempt++) {
        saw_nodes = 0;
        AtspiAccessible *desktop = atspi_get_desktop(0);
        if (desktop != NULL) {
            walk(desktop, 0);
            if (activate_name == NULL && saw_nodes) {
                return 0;
            }
            if (activate_name != NULL && activated) {
                return 0;
            }
        }
        if (attempt < 49) {
            sleep(1);
        }
    }

    if (activate_name != NULL) {
        fprintf(stderr, "a11y-probe: never found an activatable node named '%s'\n",
                activate_name);
        return 1;
    }
    return 0;
}
