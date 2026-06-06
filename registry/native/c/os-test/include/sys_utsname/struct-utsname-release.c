#include <sys/utsname.h>
void foo(struct utsname* bar)
{
	char *qux = bar->release;
	(void) qux;
}
int main(void) { return 0; }
