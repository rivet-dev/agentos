/*[MSG]*/
#include <mqueue.h>
#ifdef mq_timedreceive
#undef mq_timedreceive
#endif
ssize_t (*foo)(mqd_t, char *restrict, size_t, unsigned *restrict, const struct timespec *restrict) = mq_timedreceive;
int main(void) { return 0; }
