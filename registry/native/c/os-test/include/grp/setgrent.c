/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <grp.h>
#ifdef setgrent
#undef setgrent
#endif
void (*foo)(void) = setgrent;
int main(void) { return 0; }
