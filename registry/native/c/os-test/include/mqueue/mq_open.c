/*[MSG]*/
#include <mqueue.h>
#ifdef mq_open
#undef mq_open
#endif
mqd_t (*foo)(const char *, int, ...) = mq_open;
int main(void) { return 0; }
