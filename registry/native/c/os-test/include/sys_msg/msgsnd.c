/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/msg.h>
#ifdef msgsnd
#undef msgsnd
#endif
int (*foo)(int, const void *, size_t, int) = msgsnd;
int main(void) { return 0; }
