/*
 * Narrow visual-test helper for Redunar's controlled Vulkan fixture.
 * It resizes one X11 window whose legacy WM_NAME exactly matches the supplied
 * title. It neither synthesizes input nor remains resident after the request.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <xcb/xcb.h>

static xcb_window_t find_named_window(xcb_connection_t *connection,
                                      xcb_window_t parent,
                                      xcb_atom_t modern_name,
                                      const char *title) {
    xcb_query_tree_cookie_t tree_cookie = xcb_query_tree(connection, parent);
    xcb_query_tree_reply_t *tree =
        xcb_query_tree_reply(connection, tree_cookie, NULL);
    if (tree == NULL) {
        return XCB_WINDOW_NONE;
    }

    xcb_window_t found = XCB_WINDOW_NONE;
    int count = xcb_query_tree_children_length(tree);
    xcb_window_t *children = xcb_query_tree_children(tree);
    for (int index = 0; index < count && found == XCB_WINDOW_NONE; ++index) {
        const xcb_atom_t names[] = {modern_name, XCB_ATOM_WM_NAME};
        for (size_t name_index = 0;
             name_index < sizeof(names) / sizeof(names[0]) &&
             found == XCB_WINDOW_NONE;
             ++name_index) {
            xcb_get_property_cookie_t name_cookie = xcb_get_property(
                connection, 0, children[index], names[name_index],
                XCB_GET_PROPERTY_TYPE_ANY, 0, 1024);
            xcb_get_property_reply_t *name =
                xcb_get_property_reply(connection, name_cookie, NULL);
            if (name != NULL) {
                int length = xcb_get_property_value_length(name);
                const char *value = xcb_get_property_value(name);
                if (length == (int)strlen(title) &&
                    memcmp(value, title, (size_t)length) == 0) {
                    found = children[index];
                }
                free(name);
            }
        }
        if (found == XCB_WINDOW_NONE) {
            found = find_named_window(connection, children[index], modern_name,
                                      title);
        }
    }
    free(tree);
    return found;
}

int main(int argc, char **argv) {
    if (argc != 4) {
        fprintf(stderr, "usage: %s TITLE WIDTH HEIGHT\n", argv[0]);
        return 2;
    }
    char *width_end = NULL;
    char *height_end = NULL;
    unsigned long width = strtoul(argv[2], &width_end, 10);
    unsigned long height = strtoul(argv[3], &height_end, 10);
    if (*argv[2] == '\0' || *width_end != '\0' || *argv[3] == '\0' ||
        *height_end != '\0' || width < 320 || height < 180 || width > 3840 ||
        height > 2160) {
        fputs("dimensions must be bounded integers\n", stderr);
        return 2;
    }

    int screen_number = 0;
    xcb_connection_t *connection = xcb_connect(NULL, &screen_number);
    if (connection == NULL || xcb_connection_has_error(connection) != 0) {
        fputs("cannot connect to X11\n", stderr);
        return 1;
    }
    const xcb_setup_t *setup = xcb_get_setup(connection);
    xcb_screen_iterator_t screens = xcb_setup_roots_iterator(setup);
    for (int index = 0; index < screen_number && screens.rem != 0; ++index) {
        xcb_screen_next(&screens);
    }
    if (screens.rem == 0) {
        xcb_disconnect(connection);
        fputs("X11 screen is unavailable\n", stderr);
        return 1;
    }

    static const char modern_name_string[] = "_NET_WM_NAME";
    xcb_intern_atom_cookie_t atom_cookie = xcb_intern_atom(
        connection, 0, sizeof(modern_name_string) - 1, modern_name_string);
    xcb_intern_atom_reply_t *atom =
        xcb_intern_atom_reply(connection, atom_cookie, NULL);
    if (atom == NULL) {
        xcb_disconnect(connection);
        fputs("cannot resolve X11 window-name atom\n", stderr);
        return 1;
    }
    xcb_atom_t modern_name = atom->atom;
    free(atom);
    xcb_window_t window = find_named_window(connection, screens.data->root,
                                            modern_name, argv[1]);
    if (window == XCB_WINDOW_NONE) {
        xcb_disconnect(connection);
        fputs("matching window was not found\n", stderr);
        return 1;
    }
    uint32_t values[] = {(uint32_t)width, (uint32_t)height};
    xcb_void_cookie_t cookie = xcb_configure_window_checked(
        connection, window,
        XCB_CONFIG_WINDOW_WIDTH | XCB_CONFIG_WINDOW_HEIGHT, values);
    xcb_generic_error_t *error = xcb_request_check(connection, cookie);
    if (error != NULL) {
        free(error);
        xcb_disconnect(connection);
        fputs("window manager rejected resize request\n", stderr);
        return 1;
    }
    xcb_flush(connection);
    printf("resized_window=0x%08x width=%lu height=%lu\n", window, width,
           height);
    xcb_disconnect(connection);
    return 0;
}
