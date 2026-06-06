/*[MSG]*/
#include <mqueue.h>
#ifdef mq_receive
#undef mq_receive
#endif
ssize_t (*foo)(mqd_t, char *, size_t, unsigned *) = mq_receive;
int main(void) { return 0; }
