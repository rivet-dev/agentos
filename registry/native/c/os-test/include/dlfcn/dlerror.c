#include <dlfcn.h>
#ifdef dlerror
#undef dlerror
#endif
char *(*foo)(void) = dlerror;
int main(void) { return 0; }
