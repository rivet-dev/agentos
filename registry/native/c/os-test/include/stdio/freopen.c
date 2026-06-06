#include <stdio.h>
#ifdef freopen
#undef freopen
#endif
FILE *(*foo)(const char *restrict, const char *restrict, FILE *restrict) = freopen;
int main(void) { return 0; }
