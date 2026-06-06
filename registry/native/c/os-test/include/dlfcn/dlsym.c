#include <dlfcn.h>
#ifdef dlsym
#undef dlsym
#endif
void *(*foo)(void *restrict, const char *restrict) = dlsym;
int main(void) { return 0; }
