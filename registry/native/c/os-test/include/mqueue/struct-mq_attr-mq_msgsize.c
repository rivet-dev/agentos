/*[MSG]*/
#include <mqueue.h>
void foo(struct mq_attr* bar)
{
	long *qux = &bar->mq_msgsize;
	(void) qux;
}
int main(void) { return 0; }
