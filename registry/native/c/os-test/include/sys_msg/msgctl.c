/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/msg.h>
#ifdef msgctl
#undef msgctl
#endif
int (*foo)(int, int, struct msqid_ds *) = msgctl;
int main(void) { return 0; }
