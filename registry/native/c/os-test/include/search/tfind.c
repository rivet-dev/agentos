/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <search.h>
#ifdef tfind
#undef tfind
#endif
posix_tnode *(*foo)(const void *, posix_tnode *const *, int(*)(const void *, const void *)) = tfind;
int main(void) { return 0; }
