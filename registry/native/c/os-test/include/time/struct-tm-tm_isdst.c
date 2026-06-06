#include <time.h>
void foo(struct tm* bar)
{
	int *qux = &bar->tm_isdst;
	(void) qux;
}
int main(void) { return 0; }
