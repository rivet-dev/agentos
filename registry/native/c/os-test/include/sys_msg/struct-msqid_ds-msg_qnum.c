/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/msg.h>
void foo(struct msqid_ds* bar)
{
	msgqnum_t *qux = &bar->msg_qnum;
	(void) qux;
}
int main(void) { return 0; }
