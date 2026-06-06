/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/ipc.h>
void foo(struct ipc_perm* bar)
{
	uid_t *qux = &bar->uid;
	(void) qux;
}
int main(void) { return 0; }
