/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <utmpx.h>
#ifdef setutxent
#undef setutxent
#endif
void (*foo)(void) = setutxent;
int main(void) { return 0; }
