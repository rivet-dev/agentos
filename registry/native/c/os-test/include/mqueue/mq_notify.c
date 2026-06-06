/*[MSG]*/
#include <mqueue.h>
#ifdef mq_notify
#undef mq_notify
#endif
int (*foo)(mqd_t, const struct sigevent *) = mq_notify;
int main(void) { return 0; }
