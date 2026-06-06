/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <grp.h>
#ifdef getgrent
#undef getgrent
#endif
struct group *(*foo)(void) = getgrent;
int main(void) { return 0; }
