#include <fnmatch.h>
#ifdef fnmatch
#undef fnmatch
#endif
int (*foo)(const char *, const char *, int) = fnmatch;
int main(void) { return 0; }
