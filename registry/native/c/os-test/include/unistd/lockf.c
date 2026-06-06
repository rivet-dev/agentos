/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <unistd.h>
#ifdef lockf
#undef lockf
#endif
int (*foo)(int, int, off_t) = lockf;
int main(void) { return 0; }
