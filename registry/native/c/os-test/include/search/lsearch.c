/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <search.h>
#ifdef lsearch
#undef lsearch
#endif
void *(*foo)(const void *, void *, size_t *, size_t, int (*)(const void *, const void *)) = lsearch;
int main(void) { return 0; }
