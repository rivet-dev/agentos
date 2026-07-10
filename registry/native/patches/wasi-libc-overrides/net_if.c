#include <errno.h>
#include <net/if.h>
#include <stdlib.h>
#include <string.h>

/*
 * Linux always exposes the loopback interface. Keep this compatibility view
 * finite and deterministic until interface enumeration is part of the shared
 * typed syscall ABI. See netdevice(7) and if_nameindex(3).
 * https://man7.org/linux/man-pages/man7/netdevice.7.html
 * https://man7.org/linux/man-pages/man3/if_nameindex.3.html
 */
unsigned int if_nametoindex(const char *name) {
    if (name != NULL && strcmp(name, "lo") == 0)
        return 1;
    errno = ENXIO;
    return 0;
}

char *if_indextoname(unsigned int index, char *name) {
    if (name == NULL) {
        errno = EFAULT;
        return NULL;
    }
    if (index != 1) {
        errno = ENXIO;
        return NULL;
    }
    memcpy(name, "lo", 3);
    return name;
}

struct if_nameindex *if_nameindex(void) {
    struct if_nameindex *result = calloc(2, sizeof(*result));
    if (result == NULL)
        return NULL;
    result[0].if_name = malloc(3);
    if (result[0].if_name == NULL) {
        free(result);
        return NULL;
    }
    memcpy(result[0].if_name, "lo", 3);
    result[0].if_index = 1;
    return result;
}

void if_freenameindex(struct if_nameindex *interfaces) {
    if (interfaces == NULL)
        return;
    for (struct if_nameindex *entry = interfaces;
         entry->if_index != 0 || entry->if_name != NULL;
         entry++)
        free(entry->if_name);
    free(interfaces);
}
