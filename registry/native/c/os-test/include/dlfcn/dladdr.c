#include <dlfcn.h>
#ifdef dladdr
#undef dladdr
#endif
int (*foo)(const void *restrict, Dl_info_t *restrict) = dladdr;
int main(void) { return 0; }
