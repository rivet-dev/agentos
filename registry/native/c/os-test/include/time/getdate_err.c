/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <time.h>
void foo(void)
{
	int bar = getdate_err;
	(void) bar;
}
int main(void) { return 0; }
