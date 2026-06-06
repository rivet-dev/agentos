/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <search.h>
#ifdef hdestroy
#undef hdestroy
#endif
void (*foo)(void) = hdestroy;
int main(void) { return 0; }
