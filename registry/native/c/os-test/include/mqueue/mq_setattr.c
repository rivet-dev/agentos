/*[MSG]*/
#include <mqueue.h>
#ifdef mq_setattr
#undef mq_setattr
#endif
int (*foo)(mqd_t, const struct mq_attr *restrict, struct mq_attr *restrict) = mq_setattr;
int main(void) { return 0; }
