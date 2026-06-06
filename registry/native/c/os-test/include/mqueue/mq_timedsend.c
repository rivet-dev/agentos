/*[MSG]*/
#include <mqueue.h>
#ifdef mq_timedsend
#undef mq_timedsend
#endif
int (*foo)(mqd_t, const char *, size_t, unsigned, const struct timespec *) = mq_timedsend;
int main(void) { return 0; }
