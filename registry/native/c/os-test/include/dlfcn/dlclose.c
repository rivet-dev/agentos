#include <dlfcn.h>
#ifdef dlclose
#undef dlclose
#endif
int (*foo)(void *) = dlclose;
int main(void) { return 0; }
