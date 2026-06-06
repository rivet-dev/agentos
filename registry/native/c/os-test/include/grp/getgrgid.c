#include <grp.h>
#ifdef getgrgid
#undef getgrgid
#endif
struct group *(*foo)(gid_t) = getgrgid;
int main(void) { return 0; }
