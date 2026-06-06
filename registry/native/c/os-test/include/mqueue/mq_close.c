/*[MSG]*/
#include <mqueue.h>
#ifdef mq_close
#undef mq_close
#endif
int (*foo)(mqd_t) = mq_close;
int main(void) { return 0; }
