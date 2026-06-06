#include <stdlib.h>
#ifdef realpath
#undef realpath
#endif
char *(*foo)(const char *restrict, char *restrict) = realpath;
int main(void) { return 0; }
