/*[MSG]*/
#include <mqueue.h>
#ifdef mq_unlink
#undef mq_unlink
#endif
int (*foo)(const char *) = mq_unlink;
int main(void) { return 0; }
