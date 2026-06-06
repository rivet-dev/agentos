/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <signal.h>
void foo(siginfo_t* bar)
{
	int *qux = &bar->si_errno;
	(void) qux;
}
int main(void) { return 0; }
