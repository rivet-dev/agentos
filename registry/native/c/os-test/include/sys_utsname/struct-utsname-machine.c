#include <sys/utsname.h>
void foo(struct utsname* bar)
{
	char *qux = bar->machine;
	(void) qux;
}
int main(void) { return 0; }
