/*[MSG]*/
#include <mqueue.h>
#ifdef mq_send
#undef mq_send
#endif
int (*foo)(mqd_t, const char *, size_t, unsigned) = mq_send;
int main(void) { return 0; }
