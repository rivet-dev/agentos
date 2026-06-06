#include <time.h>
void foo(struct tm* bar)
{
	long *qux = &bar->tm_gmtoff;
	(void) qux;
}
int main(void) { return 0; }
