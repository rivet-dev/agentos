/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <unistd.h>
#ifdef crypt
#undef crypt
#endif
char *(*foo)(const char *, const char *) = crypt;
int main(void) { return 0; }
