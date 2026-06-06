/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <search.h>
#ifdef tdelete
#undef tdelete
#endif
void *(*foo)(const void *restrict, posix_tnode **restrict, int(*)(const void *, const void *)) = tdelete;
int main(void) { return 0; }
