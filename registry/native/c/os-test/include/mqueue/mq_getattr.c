/*[MSG]*/
#include <mqueue.h>
#ifdef mq_getattr
#undef mq_getattr
#endif
int (*foo)(mqd_t, struct mq_attr *) = mq_getattr;
int main(void) { return 0; }
