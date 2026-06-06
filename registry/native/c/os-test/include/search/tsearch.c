/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <search.h>
#ifdef tsearch
#undef tsearch
#endif
posix_tnode *(*foo)(const void *, posix_tnode **, int(*)(const void *, const void *)) = tsearch;
int main(void) { return 0; }
