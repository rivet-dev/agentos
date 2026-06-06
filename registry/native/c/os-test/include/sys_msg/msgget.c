/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/msg.h>
#ifdef msgget
#undef msgget
#endif
int (*foo)(key_t, int) = msgget;
int main(void) { return 0; }
