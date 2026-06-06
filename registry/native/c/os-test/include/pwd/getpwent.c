/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <pwd.h>
#ifdef getpwent
#undef getpwent
#endif
struct passwd *(*foo)(void) = getpwent;
int main(void) { return 0; }
