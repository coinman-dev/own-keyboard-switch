/* Test-only transport for the isolated KWin virtual compositor. Protocol
 * declarations are generated from the distribution's Plasma Wayland XML.
 * This helper is never linked into or shipped with Own Keyboard Switch. */
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wayland-client.h>
#include "fake-input-client.h"

static struct org_kde_kwin_fake_input *keyboard;
static uint32_t keyboard_version;
static void global(void *data, struct wl_registry *registry, uint32_t name,
                   const char *interface, uint32_t version)
{
    (void)data;
    if (!strcmp(interface, "org_kde_kwin_fake_input") && version >= 4) {
        keyboard_version = version < 5 ? version : 5;
        keyboard = wl_registry_bind(registry, name,
                                    &org_kde_kwin_fake_input_interface, keyboard_version);
    }
}
static void removed(void *data, struct wl_registry *registry, uint32_t name)
{
    (void)data; (void)registry; (void)name;
}
static const struct wl_registry_listener listener = {global, removed};

int main(int argc, char **argv)
{
    alarm(5);
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) { fputs("No private Wayland display\n", stderr); return 1; }
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &listener, NULL);
    if (wl_display_roundtrip(display) < 0 || !keyboard) {
        fputs("KWin test keyboard protocol unavailable\n", stderr); return 1;
    }
    org_kde_kwin_fake_input_authenticate(keyboard, "OKBS isolated acceptance",
                                       "Deliver only the fixture uinput output");
    org_kde_kwin_fake_input_keyboard_key(keyboard, 42, 1);
    org_kde_kwin_fake_input_keyboard_key(keyboard, 42, 0);
    if (wl_display_roundtrip(display) < 0) return 1;
    alarm(0);
    if (argc == 4 && !strcmp(argv[1], "--click")) {
        alarm(5);
        org_kde_kwin_fake_input_pointer_motion_absolute(keyboard,
            wl_fixed_from_double(strtod(argv[2], NULL)), wl_fixed_from_double(strtod(argv[3], NULL)));
        org_kde_kwin_fake_input_button(keyboard, 272, 1);
        org_kde_kwin_fake_input_button(keyboard, 272, 0);
        if (wl_display_roundtrip(display) < 0) return 1;
        goto finish;
    }
    puts("ready"); fflush(stdout);
    unsigned code, state;
    char line[128];
    while (fgets(line, sizeof(line), stdin)) {
        int x,y;
        if (sscanf(line, "p %d %d", &x, &y)==2) {
            alarm(5);
            org_kde_kwin_fake_input_pointer_motion_absolute(keyboard,wl_fixed_from_int(x),wl_fixed_from_int(y));
            org_kde_kwin_fake_input_button(keyboard,272,1);
            org_kde_kwin_fake_input_button(keyboard,272,0);
            if (wl_display_roundtrip(display)<0) return 1;
            alarm(0);
            printf("p %d %d\n",x,y); fflush(stdout); continue;
        }
        if (sscanf(line,"%u %u",&code,&state)!=2) return 1;
        if (code >= 768 || state > 1) return 1;
        alarm(5);
        org_kde_kwin_fake_input_keyboard_key(keyboard, code, state);
        if (wl_display_roundtrip(display) < 0) return 1;
        alarm(0);
        printf("%u %u\n", code, state); fflush(stdout);
    }
finish:
    if (keyboard_version >= 5) org_kde_kwin_fake_input_destroy(keyboard);
    else wl_proxy_destroy((struct wl_proxy *)keyboard);
    wl_registry_destroy(registry);
    wl_display_flush(display);
    wl_display_disconnect(display);
    return 0;
}
