/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <grp.h>
#ifdef endgrent
#undef endgrent
#endif
void (*foo)(void) = endgrent;
int main(void) { return 0; }
