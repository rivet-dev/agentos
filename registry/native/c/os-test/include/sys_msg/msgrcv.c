/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/msg.h>
#ifdef msgrcv
#undef msgrcv
#endif
ssize_t (*foo)(int, void *, size_t, long, int) = msgrcv;
int main(void) { return 0; }
