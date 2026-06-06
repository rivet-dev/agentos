/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <unistd.h>
#ifdef setresgid
#undef setresgid
#endif
int (*foo)(gid_t, gid_t, gid_t) = setresgid;
int main(void) { return 0; }
