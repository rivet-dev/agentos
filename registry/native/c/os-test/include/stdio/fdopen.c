#include <stdio.h>
#ifdef fdopen
#undef fdopen
#endif
FILE *(*foo)(int, const char *) = fdopen;
int main(void) { return 0; }
