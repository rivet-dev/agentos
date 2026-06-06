#include <dlfcn.h>
#ifdef dlopen
#undef dlopen
#endif
void *(*foo)(const char *, int) = dlopen;
int main(void) { return 0; }
