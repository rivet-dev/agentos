#include <signal.h>
void foo(struct sigevent* bar)
{
	pthread_attr_t **qux = &bar->sigev_notify_attributes;
	(void) qux;
}
int main(void) { return 0; }
