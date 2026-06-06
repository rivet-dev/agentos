/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <pwd.h>
#ifdef setpwent
#undef setpwent
#endif
void (*foo)(void) = setpwent;
int main(void) { return 0; }
