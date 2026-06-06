/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <unistd.h>
#ifdef getresgid
#undef getresgid
#endif
int (*foo)(gid_t *restrict, gid_t *restrict, gid_t *restrict) = getresgid;
int main(void) { return 0; }
