// SPDX-License-Identifier: GPL-2.0-or-later
/*
 * CPC GPIO Driver
 *
 * Copyright (C) 2023 Silicon Labs
 */

#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include <linux/gpio/driver.h>
#include <linux/list.h>
#include <linux/platform_device.h>
#include <linux/string_helpers.h>
#include <net/genetlink.h>
#include <uapi/linux/gpio.h>

/* Driver API version */
#define CPC_GPIO_API_VERSION_MAJOR 1
#define CPC_GPIO_API_VERSION_MINOR 0
#define CPC_GPIO_API_VERSION_PATCH 0

/* Driver Name */
#define CPC_GPIO_DRIVER_NAME "cpc-gpio"

/* Generic Netlink Family Name */
#define CPC_GPIO_GENL_FAMILY_NAME "CPC_GPIO_GENL"

/* Generic Netlink Multicast Family Name */
#define CPC_GPIO_GENL_MULTICAST_FAMILY_NAME "CPC_GPIO_GENL_M"
#define CPC_GPIO_GENL_MULTICAST_UID_ALL 0

/* Generic Netlink version */
#define CPC_GPIO_GENL_VERSION 1

/* Timeout while waiting on Gpio CPC Bridge */
#define CPC_GPIO_TIMEOUT_SECONDS 2
#define CPC_GPIO_TIMEOUT_MSEC (CPC_GPIO_TIMEOUT_SECONDS * 1000)

/* Maximum number of properties + the sentinel */
#define CPC_GPIO_DEVICE_MAX_PROP (4 + 1)

/* GPIO is disabled */
#define GPIO_LINE_DIRECTION_DISABLED 2

struct cpc_gpio_line {
  s32 value;
  s32 direction;
  u32 status;
  struct semaphore signal;
};

struct cpc_gpio_chip {
  u64 uid;
  struct cpc_gpio_line *lines;
  struct platform_device *pdev;
  struct gpio_chip gc;
  struct mutex lock;
};

struct cpc_gpio_chip_list_item {
  struct list_head list;
  struct cpc_gpio_chip *chip;
};

enum cpc_gpio_genl_attribute {
  CPC_GPIO_GENL_ATTR_UNSPEC,
  CPC_GPIO_GENL_ATTR_STATUS,
  CPC_GPIO_GENL_ATTR_MESSAGE,
  CPC_GPIO_GENL_ATTR_VERSION_MAJOR,
  CPC_GPIO_GENL_ATTR_VERSION_MINOR,
  CPC_GPIO_GENL_ATTR_VERSION_PATCH,
  CPC_GPIO_GENL_ATTR_UNIQUE_ID,
  CPC_GPIO_GENL_ATTR_CHIP_LABEL,
  CPC_GPIO_GENL_ATTR_GPIO_COUNT,
  CPC_GPIO_GENL_ATTR_GPIO_NAMES,
  CPC_GPIO_GENL_ATTR_GPIO_PIN,
  CPC_GPIO_GENL_ATTR_GPIO_VALUE,
  CPC_GPIO_GENL_ATTR_GPIO_CONFIG,
  CPC_GPIO_GENL_ATTR_GPIO_DIRECTION,
  __CPC_GPIO_GENL_ATTR_MAX,
};

enum cpc_gpio_genl_command {
  CPC_GPIO_GENL_CMD_UNSPEC,
  CPC_GPIO_GENL_CMD_EXIT,
  CPC_GPIO_GENL_CMD_INIT,
  CPC_GPIO_GENL_CMD_DEINIT,
  CPC_GPIO_GENL_CMD_GET_GPIO_VALUE,
  CPC_GPIO_GENL_CMD_SET_GPIO_VALUE,
  CPC_GPIO_GENL_CMD_SET_GPIO_CONFIG,
  CPC_GPIO_GENL_CMD_SET_GPIO_DIRECTION,
  __CPC_GPIO_GENL_CMD_MAX,
};

enum cpc_status_t {
  CPC_STATUS_OK = 0,
  CPC_STATUS_NOT_SUPPORTED = 1,
  CPC_STATUS_BROKEN_PIPE = 2,
  CPC_STATUS_PROTOCOL_ERROR = 3,
  CPC_STATUS_UNKNOWN = UINT_MAX,
};

static int cpc_gpio_probe(struct platform_device *pdev);

int cpc_gpio_genl_callback_init(struct sk_buff *sender_skb,
                                struct genl_info *info);
int cpc_gpio_genl_callback_deinit(struct sk_buff *sender_skb,
                                  struct genl_info *info);
int cpc_gpio_genl_callback_get_gpio_value(struct sk_buff *sender_skb,
                                          struct genl_info *info);
int cpc_gpio_genl_callback_set_gpio_value(struct sk_buff *sender_skb,
                                          struct genl_info *info);
int cpc_gpio_genl_callback_set_gpio_config(struct sk_buff *sender_skb,
                                           struct genl_info *info);
int cpc_gpio_genl_callback_set_gpio_direction(struct sk_buff *sender_skb,
                                              struct genl_info *info);

// GPIO Driver
static const struct of_device_id cpc_gpio_of_match[] = {
  {
    .compatible = CPC_GPIO_DRIVER_NAME,
  },
  {},
};
MODULE_DEVICE_TABLE(of, cpc_gpio_of_match);
static struct platform_driver cpc_gpio_driver = {
  .driver = {
    .name = CPC_GPIO_DRIVER_NAME,
    .of_match_table = cpc_gpio_of_match,
  },
  .probe = cpc_gpio_probe,
};

// GPIO Chip List
static LIST_HEAD(cpc_gpio_chip_list);

// GPIO Chip List Lock
static DEFINE_MUTEX(cpc_gpio_chip_list_lock);

static bool cpc_empty_chip_list(void)
{
  bool empty = false;

  mutex_lock(&cpc_gpio_chip_list_lock);

  empty = list_empty(&cpc_gpio_chip_list);

  mutex_unlock(&cpc_gpio_chip_list_lock);

  return empty;
}

static struct cpc_gpio_chip** cpc_find_chip(u64 uid)
{
  struct cpc_gpio_chip_list_item *list_item = NULL;
  struct cpc_gpio_chip** chip = NULL;

  mutex_lock(&cpc_gpio_chip_list_lock);

  list_for_each_entry(list_item, &cpc_gpio_chip_list, list)
  {
    if (list_item->chip->uid == uid) {
      chip = &list_item->chip;
      break;
    }
  }

  mutex_unlock(&cpc_gpio_chip_list_lock);

  return chip;
}

static int cpc_register_chip(struct cpc_gpio_chip *chip)
{
  struct cpc_gpio_chip_list_item *list_item;

  list_item = kzalloc(sizeof(*list_item), GFP_KERNEL);
  if (!list_item) {
    return -ENOMEM;
  }

  list_item->chip = chip;
  INIT_LIST_HEAD(&list_item->list);

  mutex_lock(&cpc_gpio_chip_list_lock);

  list_add(&list_item->list, &cpc_gpio_chip_list);

  mutex_unlock(&cpc_gpio_chip_list_lock);

  pr_info("%s: uid: %llu\n", __func__, chip->uid);

  return 0;
}

static void cpc_unregister_chip(struct cpc_gpio_chip **chip_ptr)
{
  struct platform_device *pdev;
  struct fwnode_handle *fwnode;
  struct cpc_gpio_chip_list_item *list_item;
  struct cpc_gpio_chip *chip = *chip_ptr;

  mutex_lock(&cpc_gpio_chip_list_lock);

  pdev = chip->pdev;
  fwnode = dev_fwnode(&pdev->dev);
  platform_device_unregister(pdev);
  fwnode_remove_software_node(fwnode);

  list_item = container_of(chip_ptr, struct cpc_gpio_chip_list_item, chip);
  list_del(&list_item->list);
  kfree(list_item);

  mutex_unlock(&cpc_gpio_chip_list_lock);

  pr_info("%s: uid: %llu\n", __func__, chip->uid);
}

static struct nla_policy cpc_gpio_genl_policy[__CPC_GPIO_GENL_ATTR_MAX] = {
  [CPC_GPIO_GENL_ATTR_UNSPEC] = { .type = NLA_UNSPEC },
  [CPC_GPIO_GENL_ATTR_STATUS] = { .type = NLA_U32 },
  [CPC_GPIO_GENL_ATTR_MESSAGE] = { .type = NLA_NUL_STRING },
  [CPC_GPIO_GENL_ATTR_VERSION_MAJOR] = { .type = NLA_U8 },
  [CPC_GPIO_GENL_ATTR_VERSION_MINOR] = { .type = NLA_U8 },
  [CPC_GPIO_GENL_ATTR_VERSION_PATCH] = { .type = NLA_U8 },
  [CPC_GPIO_GENL_ATTR_UNIQUE_ID] = { .type = NLA_U64 },
  [CPC_GPIO_GENL_ATTR_CHIP_LABEL] = { .type = NLA_NUL_STRING },
  [CPC_GPIO_GENL_ATTR_GPIO_COUNT] = { .type = NLA_U32 },
  [CPC_GPIO_GENL_ATTR_GPIO_NAMES] = { .type = NLA_NUL_STRING },
  [CPC_GPIO_GENL_ATTR_GPIO_PIN] = { .type = NLA_U32 },
  [CPC_GPIO_GENL_ATTR_GPIO_VALUE] = { .type = NLA_U32 },
  [CPC_GPIO_GENL_ATTR_GPIO_CONFIG] = { .type = NLA_U32 },
  [CPC_GPIO_GENL_ATTR_GPIO_DIRECTION] = { .type = NLA_U32 },
};

struct genl_ops cpc_gpio_genl_ops[] = {
  {
    .cmd = CPC_GPIO_GENL_CMD_INIT,
    .doit = cpc_gpio_genl_callback_init,
  },
  {
    .cmd = CPC_GPIO_GENL_CMD_DEINIT,
    .doit = cpc_gpio_genl_callback_deinit,
  },
  {
    .cmd = CPC_GPIO_GENL_CMD_GET_GPIO_VALUE,
    .doit = cpc_gpio_genl_callback_get_gpio_value,
  },
  {
    .cmd = CPC_GPIO_GENL_CMD_SET_GPIO_VALUE,
    .doit = cpc_gpio_genl_callback_set_gpio_value,
  },
  {
    .cmd = CPC_GPIO_GENL_CMD_SET_GPIO_CONFIG,
    .doit = cpc_gpio_genl_callback_set_gpio_config,
  },
  {
    .cmd = CPC_GPIO_GENL_CMD_SET_GPIO_DIRECTION,
    .doit = cpc_gpio_genl_callback_set_gpio_direction,
  }
};

static struct genl_multicast_group cpc_gpio_genl_family_mc[] = {
  { .name = CPC_GPIO_GENL_MULTICAST_FAMILY_NAME }
};

static struct genl_family cpc_gpio_genl_family = {
  .id = 0,
  .hdrsize = 0,
  .name = CPC_GPIO_GENL_FAMILY_NAME,
  .version = CPC_GPIO_GENL_VERSION,
  .ops = cpc_gpio_genl_ops,
  .n_ops = ARRAY_SIZE(cpc_gpio_genl_ops),
  .policy = cpc_gpio_genl_policy,
  .maxattr = __CPC_GPIO_GENL_ATTR_MAX,
  .module = THIS_MODULE,
  .parallel_ops = 0,
  .netnsok = 0,
  .pre_doit = NULL,
  .post_doit = NULL,
  .mcgrps = cpc_gpio_genl_family_mc,
  .n_mcgrps = ARRAY_SIZE(cpc_gpio_genl_family_mc)
};

static int cpc_status_to_errno(enum cpc_status_t status)
{
  switch (status) {
    case CPC_STATUS_OK:
      return 0;
    case CPC_STATUS_NOT_SUPPORTED:
      return -EOPNOTSUPP;
    case CPC_STATUS_BROKEN_PIPE:
      return -EPIPE;
    case CPC_STATUS_PROTOCOL_ERROR:
      return -EPROTO;
    case CPC_STATUS_UNKNOWN:
      return -EIO;
    default:
      return -EIO;
  }
}

static int cpc_gpio_register_chip(u64 uid, char* chip_label, u16 ngpio, char** gpio_names)
{
  struct property_entry properties[CPC_GPIO_DEVICE_MAX_PROP];
  struct platform_device_info pdevinfo;
  struct fwnode_handle *fwnode;
  int prop = 0;
  int err = 0;

  if (!cpc_empty_chip_list()) {
    pr_err("%s: only one chip can be registered\n", __func__);
    err = -EPERM;
    return err;
  }

  memset(properties, 0, sizeof(properties));
  memset(&pdevinfo, 0, sizeof(pdevinfo));

  // Unique Id
  properties[prop++] = PROPERTY_ENTRY_U64("uid", uid);

  // Chip label
  properties[prop++] = PROPERTY_ENTRY_STRING("chip-label", chip_label);

  // GPIO count: "ngpios" is a reserved property
  properties[prop++] = PROPERTY_ENTRY_U16("ngpios", ngpio);

  // GPIO names: "gpio-line-names" is a reserved property
  properties[prop++] = PROPERTY_ENTRY_STRING_ARRAY_LEN("gpio-line-names",
                                                       gpio_names, ngpio);

  fwnode = fwnode_create_software_node(properties, NULL);
  err = IS_ERR(fwnode);
  if (err) {
    pr_err("%s: fwnode_create_software_node failed: %d\n", __func__, err);
    return err;
  }

  pdevinfo.name = CPC_GPIO_DRIVER_NAME;
  pdevinfo.id = PLATFORM_DEVID_AUTO;
  pdevinfo.fwnode = fwnode;

  // `cpc_gpio_probe` is called within `platform_device_register_full`
  err = IS_ERR(platform_device_register_full(&pdevinfo));
  if (err) {
    pr_err("%s: platform_device_register_full failed: %d\n", __func__, err);
    fwnode_remove_software_node(fwnode);
    return err;
  }

  return err;
}

static int cpc_gpio_multicast_get_gpio_value(u64 uid, unsigned int pin)
{
  int rc;
  int ret = 0;
  struct sk_buff *skb;
  void *genl_msg;

  skb = nlmsg_new(NLMSG_GOODSIZE, GFP_KERNEL);
  if (!skb) {
    pr_err("%s: nlmsg_new failed\n", __func__);
    ret = -1;
    goto done;
  }

  genl_msg = genlmsg_put(skb, 0, 0,
                         &cpc_gpio_genl_family, 0,
                         CPC_GPIO_GENL_CMD_GET_GPIO_VALUE);
  if (!genl_msg) {
    pr_err("%s: genlmsg_put failed\n", __func__);
    ret = -1;
    goto done;
  }

  rc = nla_put_u64_64bit(skb, CPC_GPIO_GENL_ATTR_UNIQUE_ID, uid, CPC_GPIO_GENL_ATTR_UNSPEC);
  if (rc != 0) {
    pr_err("%s: nla_put_u64_64bit failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  rc = nla_put_u32(skb, CPC_GPIO_GENL_ATTR_GPIO_PIN, pin);
  if (rc != 0) {
    pr_err("%s: nla_put_u32 failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  genlmsg_end(skb, genl_msg);
  rc = genlmsg_multicast(&cpc_gpio_genl_family, skb, 0, 0, GFP_KERNEL);
  skb = NULL;

  if (rc != 0 && rc != -ESRCH) {
    pr_err("%s: genlmsg_multicast failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  done:
  if (skb) {
    nlmsg_free(skb);
    skb = NULL;
  }

  return ret;
}

static int cpc_gpio_multicast_set_gpio_value(u64 uid, unsigned int pin,
                                             unsigned int value)
{
  int rc;
  int ret = 0;
  struct sk_buff *skb;
  void *genl_msg;

  skb = nlmsg_new(NLMSG_GOODSIZE, GFP_KERNEL);
  if (!skb) {
    pr_err("%s: nlmsg_new failed\n", __func__);
    ret = -1;
    goto done;
  }

  genl_msg = genlmsg_put(skb, 0, 0,
                         &cpc_gpio_genl_family, 0,
                         CPC_GPIO_GENL_CMD_SET_GPIO_VALUE);
  if (!genl_msg) {
    pr_err("%s: genlmsg_put failed\n", __func__);
    ret = -1;
    goto done;
  }

  rc = nla_put_u64_64bit(skb, CPC_GPIO_GENL_ATTR_UNIQUE_ID, uid, CPC_GPIO_GENL_ATTR_UNSPEC);
  if (rc != 0) {
    pr_err("%s: nla_put_u64_64bit failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  rc = nla_put_u32(skb, CPC_GPIO_GENL_ATTR_GPIO_PIN, pin);
  if (rc != 0) {
    pr_err("%s: nla_put_u32 failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  rc = nla_put_u32(skb, CPC_GPIO_GENL_ATTR_GPIO_VALUE, value);
  if (rc != 0) {
    pr_err("%s: nla_put_u32 failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  genlmsg_end(skb, genl_msg);
  rc = genlmsg_multicast(&cpc_gpio_genl_family, skb, 0, 0, GFP_KERNEL);
  skb = NULL;

  if (rc != 0 && rc != -ESRCH) {
    pr_err("%s: genlmsg_multicast failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  done:
  if (skb) {
    nlmsg_free(skb);
    skb = NULL;
  }

  return ret;
}

static int cpc_gpio_multicast_set_gpio_config(u64 uid, unsigned int pin, unsigned int config)
{
  int rc;
  int ret = 0;
  struct sk_buff *skb;
  void *genl_msg;

  skb = nlmsg_new(NLMSG_GOODSIZE, GFP_KERNEL);
  if (!skb) {
    pr_err("%s: nlmsg_new failed\n", __func__);
    ret = -1;
    goto done;
  }

  genl_msg = genlmsg_put(skb, 0, 0,
                         &cpc_gpio_genl_family, 0,
                         CPC_GPIO_GENL_CMD_SET_GPIO_CONFIG);
  if (!genl_msg) {
    pr_err("%s: genlmsg_put failed\n", __func__);
    ret = -1;
    goto done;
  }

  rc = nla_put_u64_64bit(skb, CPC_GPIO_GENL_ATTR_UNIQUE_ID, uid, CPC_GPIO_GENL_ATTR_UNSPEC);
  if (rc != 0) {
    pr_err("%s: nla_put_u64_64bit failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  rc = nla_put_u32(skb, CPC_GPIO_GENL_ATTR_GPIO_PIN, pin);
  if (rc != 0) {
    pr_err("%s: nla_put_u32 failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  rc = nla_put_u32(skb, CPC_GPIO_GENL_ATTR_GPIO_CONFIG, config);
  if (rc != 0) {
    pr_err("%s: nla_put_u32 failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  genlmsg_end(skb, genl_msg);
  rc = genlmsg_multicast(&cpc_gpio_genl_family, skb, 0, 0, GFP_KERNEL);
  skb = NULL;

  if (rc != 0 && rc != -ESRCH) {
    pr_err("%s: genlmsg_multicast failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  done:
  if (skb) {
    nlmsg_free(skb);
    skb = NULL;
  }

  return ret;
}

static int cpc_gpio_multicast_set_gpio_direction(u64 uid, unsigned int pin, unsigned int direction)
{
  int rc;
  int ret = 0;
  struct sk_buff *skb;
  void *genl_msg;

  skb = nlmsg_new(NLMSG_GOODSIZE, GFP_KERNEL);
  if (!skb) {
    pr_err("%s: nlmsg_new failed\n", __func__);
    ret = -1;
    goto done;
  }

  genl_msg = genlmsg_put(skb, 0, 0,
                         &cpc_gpio_genl_family, 0,
                         CPC_GPIO_GENL_CMD_SET_GPIO_DIRECTION);
  if (!genl_msg) {
    pr_err("%s: genlmsg_put failed\n", __func__);
    ret = -1;
    goto done;
  }

  rc = nla_put_u64_64bit(skb, CPC_GPIO_GENL_ATTR_UNIQUE_ID, uid, CPC_GPIO_GENL_ATTR_UNSPEC);
  if (rc != 0) {
    pr_err("%s: nla_put_u64_64bit failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  rc = nla_put_u32(skb, CPC_GPIO_GENL_ATTR_GPIO_PIN, pin);
  if (rc != 0) {
    pr_err("%s: nla_put_u32 failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  rc = nla_put_u32(skb, CPC_GPIO_GENL_ATTR_GPIO_DIRECTION, direction);
  if (rc != 0) {
    pr_err("%s: nla_put_u32 failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  genlmsg_end(skb, genl_msg);
  rc = genlmsg_multicast(&cpc_gpio_genl_family, skb, 0, 0, GFP_KERNEL);
  skb = NULL;

  if (rc != 0 && rc != -ESRCH) {
    pr_err("%s: genlmsg_multicast failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  done:
  if (skb) {
    nlmsg_free(skb);
    skb = NULL;
  }

  return ret;
}

static int cpc_gpio_multicast_exit(const char *exit_message)
{
  int rc;
  int ret = 0;
  struct sk_buff *skb;
  void *genl_msg;

  skb = nlmsg_new(NLMSG_GOODSIZE, GFP_KERNEL);
  if (!skb) {
    pr_err("%s: nlmsg_new failed\n", __func__);
    ret = -1;
    goto done;
  }

  genl_msg =
    genlmsg_put(skb, 0, 0,
                &cpc_gpio_genl_family, 0, CPC_GPIO_GENL_CMD_EXIT);
  if (!genl_msg) {
    pr_err("%s: genlmsg_put failed\n", __func__);
    ret = -1;
    goto done;
  }

  rc = nla_put_u64_64bit(skb, CPC_GPIO_GENL_ATTR_UNIQUE_ID, CPC_GPIO_GENL_MULTICAST_UID_ALL, CPC_GPIO_GENL_ATTR_UNSPEC);
  if (rc != 0) {
    pr_err("%s: nla_put_u64_64bit failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  rc = nla_put_string(skb, CPC_GPIO_GENL_ATTR_MESSAGE, exit_message);
  if (rc != 0) {
    pr_err("%s: nla_put_u32 failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  genlmsg_end(skb, genl_msg);
  rc = genlmsg_multicast(&cpc_gpio_genl_family, skb, 0, 0, GFP_KERNEL);
  skb = NULL;

  if (rc != 0 && rc != -ESRCH) {
    pr_err("%s: genlmsg_multicast failed: %d\n", __func__, rc);
    ret = rc;
    goto done;
  }

  done:
  if (skb) {
    nlmsg_free(skb);
    skb = NULL;
  }

  return ret;
}

int cpc_gpio_genl_callback_init(struct sk_buff *sender_skb,
                                struct genl_info *info)
{
  struct nlattr *na = NULL;
  struct sk_buff *reply_skb = NULL;
  void *msg_head = NULL;
  char **gpio_names = NULL;
  char *chip_label = NULL;
  int i = 0;
  u32 gpio_count = 0;
  s32 err = 0;
  u64 uid = 0;

  pr_debug("%s\n", __func__);

  if (!info) {
    pr_err("%s: info is NULL\n", __func__);
    err = -EINVAL;
    goto done;
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_UNIQUE_ID];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_UNIQUE_ID);
    err = -EINVAL;
    goto done;
  } else {
    uid = nla_get_u64(na);
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_GPIO_COUNT];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_GPIO_COUNT);
    err = -EINVAL;
    goto done;
  } else {
    gpio_count = nla_get_u32(na);
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_GPIO_NAMES];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_GPIO_NAMES);
    err = -EINVAL;
    goto done;
  } else {
    size_t len = 0;
    int gpio_name_count = 0;

    char *raw_names = (char *) nla_data(na);
    gpio_names = kcalloc(gpio_count, sizeof(char *), GFP_KERNEL);
    if (!gpio_names) {
      pr_err("%s: kcalloc failed\n", __func__);
      err = -ENOMEM;
      goto done;
    }

    for (i = 0; i < gpio_count; i++) {
      len = strlen(raw_names) + 1;
      gpio_names[gpio_name_count] = raw_names;
      raw_names = raw_names + len;
      gpio_name_count++;
    }

    if (gpio_count != gpio_name_count) {
      pr_err("%s: gpio_count != gpio_name_count\n", __func__);
      err = -EINVAL;
      goto done;
    }
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_CHIP_LABEL];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_CHIP_LABEL);
    err = -EINVAL;
    goto done;
  } else {
    chip_label = nla_data(na);
  }

  // Register chip
  err = cpc_gpio_register_chip(uid, chip_label, gpio_count, gpio_names);
  if (err) {
    goto done;
  }

  done:
  // 0) Cleanup
  kfree(gpio_names);

  // 1) Prepare message.
  reply_skb = genlmsg_new(NLMSG_GOODSIZE, GFP_KERNEL);
  if (!reply_skb) {
    pr_err("%s: genlmsg_new failed\n", __func__);
    err = -ENOMEM;
    goto genl_error;
  }

  msg_head =
    genlmsg_put(reply_skb, info->snd_portid, info->snd_seq,
                &cpc_gpio_genl_family, 0, CPC_GPIO_GENL_CMD_INIT);
  if (!msg_head) {
    pr_err("%s: genlmsg_put failed\n", __func__);
    err = -ENOMEM;
    goto genl_error;
  }

  // 2) Set message.
  err = nla_put_u32(reply_skb, CPC_GPIO_GENL_ATTR_STATUS, -err);
  if (err != 0) {
    pr_err("%s: nla_put_u32 failed: %d\n", __func__, err);
    goto genl_error;
  }
  genlmsg_end(reply_skb, msg_head);

  // 3) Send message.
  err = genlmsg_reply(reply_skb, info);
  reply_skb = NULL;
  if (err != 0) {
    pr_err("%s: genlmsg_reply failed: %d\n", __func__, err);
    goto genl_error;
  }

  genl_error:
  if (reply_skb) {
    nlmsg_free(reply_skb);
    reply_skb = NULL;
  }

  return err;
}

int cpc_gpio_genl_callback_deinit(struct sk_buff *sender_skb,
                                  struct genl_info *info)
{
  struct cpc_gpio_chip **chip_ptr = NULL;
  struct cpc_gpio_chip *chip = NULL;
  struct nlattr *na = NULL;
  struct sk_buff *reply_skb = NULL;
  void *msg_head = NULL;
  int i = 0;
  s32 err = 0;
  s32 nl_err = 0;
  u64 uid = 0;

  pr_debug("%s\n", __func__);

  na = info->attrs[CPC_GPIO_GENL_ATTR_UNIQUE_ID];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_UNIQUE_ID);
    err = -EINVAL;
    goto done;
  } else {
    uid = nla_get_u64(na);
  }

  chip_ptr = cpc_find_chip(uid);
  if (chip_ptr) {
    chip = *chip_ptr;
    for (i = 0; i < chip->gc.ngpio; i++) {
      if (gpiochip_is_requested(&chip->gc, i)) {
        pr_err("%s: failed to deinit, gpio pin %d is still requested\n",
               __func__, i);
        err = -EBUSY;
        goto done;
      }
    }

    cpc_unregister_chip(chip_ptr);
  }

  done:
  // 1) Prepare message.
  reply_skb = genlmsg_new(NLMSG_GOODSIZE, GFP_KERNEL);
  if (!reply_skb) {
    pr_err("%s: genlmsg_new failed\n", __func__);
    err = -ENOMEM;
    goto genl_error;
  }

  msg_head =
    genlmsg_put(reply_skb, info->snd_portid, info->snd_seq,
                &cpc_gpio_genl_family, 0, CPC_GPIO_GENL_CMD_DEINIT);
  if (!msg_head) {
    pr_err("%s: genlmsg_put failed\n", __func__);
    err = -ENOMEM;
    goto genl_error;
  }

  // 2) Set message.
  nl_err = nla_put_u8(reply_skb, CPC_GPIO_GENL_ATTR_VERSION_MAJOR, CPC_GPIO_API_VERSION_MAJOR);
  if (nl_err != 0) {
    pr_err("%s: nla_put_u8 failed: %d\n", __func__, nl_err);
    err = nl_err;
    goto genl_error;
  }

  nl_err = nla_put_u8(reply_skb, CPC_GPIO_GENL_ATTR_VERSION_MINOR, CPC_GPIO_API_VERSION_MINOR);
  if (nl_err != 0) {
    pr_err("%s: nla_put_u8 failed: %d\n", __func__, nl_err);
    err = nl_err;
    goto genl_error;
  }

  nl_err = nla_put_u8(reply_skb, CPC_GPIO_GENL_ATTR_VERSION_PATCH, CPC_GPIO_API_VERSION_PATCH);
  if (nl_err != 0) {
    pr_err("%s: nla_put_u8 failed: %d\n", __func__, nl_err);
    err = nl_err;
    goto genl_error;
  }

  nl_err = nla_put_u32(reply_skb, CPC_GPIO_GENL_ATTR_STATUS, -err);
  if (nl_err != 0) {
    pr_err("%s: nla_put_s32 failed: %d\n", __func__, nl_err);
    err = nl_err;
    goto genl_error;
  }

  genlmsg_end(reply_skb, msg_head);

  // 3) Send message.
  err = genlmsg_reply(reply_skb, info);
  reply_skb = NULL;
  if (err != 0) {
    pr_err("%s: genlmsg_reply failed: %d\n", __func__, err);
    goto genl_error;
  }

  genl_error:
  if (reply_skb) {
    nlmsg_free(reply_skb);
    reply_skb = NULL;
  }

  return err;
}

int cpc_gpio_genl_callback_get_gpio_value(struct sk_buff *sender_skb,
                                          struct genl_info *info)
{
  struct cpc_gpio_chip **chip_ptr;
  struct cpc_gpio_chip *chip;
  struct nlattr *na;
  u32 gpio_pin;
  u32 gpio_value;
  u32 status;
  u64 uid;

  if (!info) {
    pr_err("%s: info is NULL\n", __func__);
    return -EINVAL;
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_UNIQUE_ID];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_UNIQUE_ID);
    return -EINVAL;
  } else {
    uid = nla_get_u64(na);
  }

  chip_ptr = cpc_find_chip(uid);
  if (!chip_ptr) {
    pr_err("%s: chip not found (uid: %llu)\n", __func__, uid);
    return -EINVAL;
  } else {
    chip = *chip_ptr;
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_GPIO_PIN];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_GPIO_PIN);
    return -EINVAL;
  } else {
    gpio_pin = nla_get_u32(na);
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_STATUS];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_STATUS);
    return -EINVAL;
  } else {
    status = nla_get_u32(na);
  }

  chip->lines[gpio_pin].status = status;

  if (status == CPC_STATUS_OK) {
    na = info->attrs[CPC_GPIO_GENL_ATTR_GPIO_VALUE];
    if (!na) {
      pr_err("%s: No info->attrs[%d]\n", __func__,
             CPC_GPIO_GENL_ATTR_GPIO_PIN);
      return -EINVAL;
    } else {
      gpio_value = nla_get_u32(na);
    }
    chip->lines[gpio_pin].value = !!gpio_value;
  }

  up(&chip->lines[gpio_pin].signal);

  return 0;
}

int cpc_gpio_genl_callback_set_gpio_value(struct sk_buff *sender_skb,
                                          struct genl_info *info)
{
  struct cpc_gpio_chip **chip_ptr = NULL;
  struct cpc_gpio_chip *chip = NULL;
  struct nlattr *na;
  u32 gpio_pin;
  u32 status;
  u64 uid;

  if (!info) {
    pr_err("%s: info is NULL\n", __func__);
    return -EINVAL;
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_UNIQUE_ID];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_UNIQUE_ID);
    return -EINVAL;
  } else {
    uid = nla_get_u64(na);
  }

  chip_ptr = cpc_find_chip(uid);
  if (!chip_ptr) {
    pr_err("%s: chip not found (uid: %llu)\n", __func__, uid);
    return -EINVAL;
  } else {
    chip = *chip_ptr;
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_GPIO_PIN];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_GPIO_PIN);
    return -EINVAL;
  } else {
    gpio_pin = nla_get_u32(na);
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_STATUS];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_STATUS);
    return -EINVAL;
  } else {
    status = nla_get_u32(na);
  }

  chip->lines[gpio_pin].status = status;

  up(&chip->lines[gpio_pin].signal);

  return 0;
}

int cpc_gpio_genl_callback_set_gpio_config(struct sk_buff *sender_skb,
                                           struct genl_info *info)
{
  struct cpc_gpio_chip **chip_ptr = NULL;
  struct cpc_gpio_chip *chip = NULL;
  struct nlattr *na;
  u32 gpio_pin;
  s32 status;
  u64 uid;

  if (!info) {
    pr_err("%s: info is NULL\n", __func__);
    return -EINVAL;
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_UNIQUE_ID];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_UNIQUE_ID);
    return -EINVAL;
  } else {
    uid = nla_get_u64(na);
  }

  chip_ptr = cpc_find_chip(uid);
  if (!chip_ptr) {
    pr_err("%s: chip not found (uid: %llu)\n", __func__, uid);
    return -EINVAL;
  } else {
    chip = *chip_ptr;
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_GPIO_PIN];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_GPIO_PIN);
    return -EINVAL;
  } else {
    gpio_pin = nla_get_u32(na);
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_STATUS];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_STATUS);
    return -EINVAL;
  } else {
    status = nla_get_u32(na);
  }

  chip->lines[gpio_pin].status = status;

  up(&chip->lines[gpio_pin].signal);

  return 0;
}

int cpc_gpio_genl_callback_set_gpio_direction(struct sk_buff *sender_skb,
                                              struct genl_info *info)
{
  struct cpc_gpio_chip **chip_ptr = NULL;
  struct cpc_gpio_chip *chip = NULL;
  struct nlattr *na;
  u32 gpio_pin;
  s32 status;
  u64 uid;

  if (!info) {
    pr_err("%s: info is NULL\n", __func__);
    return -EINVAL;
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_UNIQUE_ID];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_UNIQUE_ID);
    return -EINVAL;
  } else {
    uid = nla_get_u64(na);
  }

  chip_ptr = cpc_find_chip(uid);
  if (!chip_ptr) {
    pr_err("%s: chip not found (uid: %llu)\n", __func__, uid);
    return -EINVAL;
  } else {
    chip = *chip_ptr;
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_GPIO_PIN];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_GPIO_PIN);
    return -EINVAL;
  } else {
    gpio_pin = nla_get_u32(na);
  }

  na = info->attrs[CPC_GPIO_GENL_ATTR_STATUS];
  if (!na) {
    pr_err("%s: No info->attrs[%d]\n", __func__,
           CPC_GPIO_GENL_ATTR_STATUS);
    return -EINVAL;
  } else {
    status = nla_get_u32(na);
  }

  chip->lines[gpio_pin].status = status;

  up(&chip->lines[gpio_pin].signal);

  return 0;
}

static int __cpc_gpio_get(struct cpc_gpio_chip *chip, unsigned int pin)
{
  int ret = -EPIPE;

  cpc_gpio_multicast_get_gpio_value(chip->uid, pin);

  if (down_timeout(&chip->lines[pin].signal,
                   CPC_GPIO_TIMEOUT_SECONDS * HZ) != 0) {
    pr_err("%s: cpc-gpio-bridge is unresponsive\n", __func__);
  } else {
    pr_debug("%s: pin = %d, value = %d, status = %d\n", __func__, pin,
             chip->lines[pin].value, chip->lines[pin].status);
    ret = cpc_status_to_errno(chip->lines[pin].status);
    if (ret == CPC_STATUS_OK) {
      ret = chip->lines[pin].value;
    }
  }

  return ret;
}

static int cpc_gpio_get(struct gpio_chip *gc, unsigned int pin)
{
  struct cpc_gpio_chip *chip = gpiochip_get_data(gc);
  int value;

  mutex_lock(&chip->lock);
  value = __cpc_gpio_get(chip, pin);
  mutex_unlock(&chip->lock);

  return value;
}

static int __cpc_gpio_set(struct cpc_gpio_chip *chip, unsigned int pin,
                          int value)
{
  int ret = -EPIPE;

  cpc_gpio_multicast_set_gpio_value(chip->uid, pin, value);

  if (down_timeout(&chip->lines[pin].signal,
                   CPC_GPIO_TIMEOUT_SECONDS * HZ) != 0) {
    pr_err("%s: cpc-gpio-bridge is unresponsive\n", __func__);
  } else {
    chip->lines[pin].value = value;
    pr_debug("%s: pin = %d, value = %d, status = %d\n", __func__, pin,
             chip->lines[pin].value, chip->lines[pin].status);
    ret = cpc_status_to_errno(chip->lines[pin].status);
  }

  return ret;
}

static void cpc_gpio_set(struct gpio_chip *gc, unsigned int pin, int value)
{
  struct cpc_gpio_chip *chip = gpiochip_get_data(gc);

  mutex_lock(&chip->lock);
  __cpc_gpio_set(chip, pin, value);
  mutex_unlock(&chip->lock);
}

static int __cpc_gpio_set_config(struct cpc_gpio_chip *chip, unsigned int pin,
                                 int config)
{
  int ret = -EPIPE;

  cpc_gpio_multicast_set_gpio_config(chip->uid, pin, config);

  if (down_timeout(&chip->lines[pin].signal,
                   CPC_GPIO_TIMEOUT_SECONDS * HZ) != 0) {
    pr_err("%s: cpc-gpio-bridge is unresponsive\n", __func__);
  } else {
    pr_debug("%s: pin = %d, config = %d, status = %d\n", __func__, pin,
             config, chip->lines[pin].status);
    ret = cpc_status_to_errno(chip->lines[pin].status);
  }

  return ret;
}

static int _cpc_gpio_set_config(struct gpio_chip *gc, unsigned int pin,
                                int value)
{
  struct cpc_gpio_chip *chip = gpiochip_get_data(gc);
  int err;

  mutex_lock(&chip->lock);
  err = __cpc_gpio_set_config(chip, pin, value);
  mutex_unlock(&chip->lock);

  return err;
}

static int cpc_gpio_set_config(struct gpio_chip *gc, unsigned int pin,
                               unsigned long config)
{
  enum pin_config_param config_param = pinconf_to_config_param(config);

  switch (config_param) {
    case PIN_CONFIG_BIAS_DISABLE:
      return _cpc_gpio_set_config(gc, pin, config_param);
    case PIN_CONFIG_BIAS_PULL_DOWN:
      return _cpc_gpio_set_config(gc, pin, config_param);
    case PIN_CONFIG_BIAS_PULL_UP:
      return _cpc_gpio_set_config(gc, pin, config_param);

    case PIN_CONFIG_DRIVE_OPEN_DRAIN:
      return _cpc_gpio_set_config(gc, pin, config_param);
    case PIN_CONFIG_DRIVE_OPEN_SOURCE:
      return _cpc_gpio_set_config(gc, pin, config_param);
    case PIN_CONFIG_DRIVE_PUSH_PULL:
      return _cpc_gpio_set_config(gc, pin, config_param);

    case PIN_CONFIG_PERSIST_STATE:
      return 0;
    default:
      break;
  }

  return -EOPNOTSUPP;
}

static int cpc_gpio_direction_disabled(struct gpio_chip *gc, unsigned int pin)
{
  int ret = -EPIPE;
  int direction = GPIO_LINE_DIRECTION_DISABLED;
  struct cpc_gpio_chip *chip = gpiochip_get_data(gc);

  mutex_lock(&chip->lock);

  cpc_gpio_multicast_set_gpio_direction(chip->uid, pin, direction);

  if (down_timeout(&chip->lines[pin].signal,
                   CPC_GPIO_TIMEOUT_SECONDS * HZ) != 0) {
    pr_err("%s: cpc-gpio-bridge is unresponsive\n", __func__);
  } else {
    chip->lines[pin].direction = direction;
    pr_debug("%s: pin = %d, direction = %d, status = %d\n", __func__, pin,
             chip->lines[pin].direction, chip->lines[pin].status);
    ret = cpc_status_to_errno(chip->lines[pin].status);
  }

  mutex_unlock(&chip->lock);

  return ret;
}

static int cpc_gpio_direction_output(struct gpio_chip *gc, unsigned int pin, int value)
{
  int ret = -EPIPE;
  int direction = GPIO_LINE_DIRECTION_OUT;
  struct cpc_gpio_chip *chip = gpiochip_get_data(gc);

  mutex_lock(&chip->lock);

  cpc_gpio_multicast_set_gpio_direction(chip->uid, pin, direction);

  if (down_timeout(&chip->lines[pin].signal,
                   CPC_GPIO_TIMEOUT_SECONDS * HZ) != 0) {
    pr_err("%s: cpc-gpio-bridge is unresponsive\n", __func__);
  } else {
    int ret_gpio_direction;
    chip->lines[pin].direction = direction;
    pr_debug("%s: pin = %d, direction = %d, status = %d\n", __func__, pin,
             chip->lines[pin].direction, chip->lines[pin].status);
    ret_gpio_direction = cpc_status_to_errno(chip->lines[pin].status);
    if (ret_gpio_direction != CPC_STATUS_OK) {
      ret = ret_gpio_direction;
    } else {
      ret = __cpc_gpio_set(chip, pin, value);
    }
  }

  mutex_unlock(&chip->lock);

  return ret;
}

static int cpc_gpio_direction_input(struct gpio_chip *gc, unsigned int pin)
{
  int ret = -EPIPE;
  int direction = GPIO_LINE_DIRECTION_IN;
  struct cpc_gpio_chip *chip = gpiochip_get_data(gc);

  mutex_lock(&chip->lock);

  cpc_gpio_multicast_set_gpio_direction(chip->uid, pin, direction);

  if (down_timeout(&chip->lines[pin].signal,
                   CPC_GPIO_TIMEOUT_SECONDS * HZ) != 0) {
    pr_err("%s: cpc-gpio-bridge is unresponsive\n", __func__);
  } else {
    chip->lines[pin].direction = direction;
    pr_debug("%s: pin = %d, direction = %d, status = %d\n", __func__, pin,
             chip->lines[pin].direction, chip->lines[pin].status);
    ret = cpc_status_to_errno(chip->lines[pin].status);
  }

  mutex_unlock(&chip->lock);

  return ret;
}

static int cpc_gpio_get_direction(struct gpio_chip *gc, unsigned int pin)
{
  struct cpc_gpio_chip *chip = gpiochip_get_data(gc);
  int direction;

  mutex_lock(&chip->lock);
  direction = chip->lines[pin].direction;
  mutex_unlock(&chip->lock);

  return direction;
}

static void cpc_gpio_free(struct gpio_chip *gc, unsigned int pin)
{
  pr_debug("%s\n", __func__);
  cpc_gpio_direction_disabled(gc, pin);
}

static int cpc_gpio_probe(struct platform_device *pdev)
{
  struct cpc_gpio_chip *chip;
  struct gpio_chip *gc;
  struct device *dev;
  const char *name;
  int rv, i;
  u16 ngpio;
  u64 uid;

  pr_debug("%s\n", __func__);

  dev = &pdev->dev;

  rv = device_property_read_u64(dev, "uid", &uid);
  if (rv) {
    return rv;
  }

  rv = device_property_read_u16(dev, "ngpios", &ngpio);
  if (rv) {
    return rv;
  }

  rv = device_property_read_string(dev, "chip-label", &name);
  if (rv) {
    name = dev_name(dev);
  }

  chip = devm_kzalloc(dev, sizeof(*chip), GFP_KERNEL);
  if (!chip) {
    return -ENOMEM;
  }

  // Context
  chip->uid = uid;
  chip->pdev = pdev;

  mutex_init(&chip->lock);

  gc = &chip->gc;
  gc->base = -1;
  gc->ngpio = ngpio;
  gc->label = name;
  gc->owner = THIS_MODULE;
  gc->parent = dev;
  gc->get = cpc_gpio_get;
  gc->set = cpc_gpio_set;
  gc->direction_output = cpc_gpio_direction_output;
  gc->direction_input = cpc_gpio_direction_input;
  gc->get_direction = cpc_gpio_get_direction;
  gc->set_config = cpc_gpio_set_config;
  gc->free = cpc_gpio_free;

  chip->lines =
    devm_kcalloc(dev, gc->ngpio, sizeof(*chip->lines), GFP_KERNEL);
  if (!chip->lines) {
    return -ENOMEM;
  }

  for (i = 0; i < gc->ngpio; i++) {
    chip->lines[i].direction = GPIO_LINE_DIRECTION_IN;
    sema_init(&chip->lines[i].signal, 0);
  }

  rv = devm_gpiochip_add_data(dev, &chip->gc, chip);
  if (rv) {
    return rv;
  }

  return cpc_register_chip(chip);
}

static int __init cpc_gpio_init(void)
{
  int err;

  pr_info("%s: Driver API v%d.%d.%d, GENL API v%d\n", __func__, CPC_GPIO_API_VERSION_MAJOR,
          CPC_GPIO_API_VERSION_MINOR,
          CPC_GPIO_API_VERSION_PATCH,
          CPC_GPIO_GENL_VERSION);

  err = platform_driver_register(&cpc_gpio_driver);
  if (err) {
    pr_err("%s: platform_driver_register failed: %d\n", __func__,
           err);
    goto done;
  }

  err = genl_register_family(&cpc_gpio_genl_family);
  if (err) {
    pr_err("%s: genl_register_family failed: %d\n", __func__, err);
    goto done;
  }

  done:
  return err;
}

static void __exit cpc_gpio_exit(void)
{
  int err;
  struct cpc_gpio_chip_list_item *list_item = NULL;
  struct cpc_gpio_chip_list_item *list_item_tmp = NULL;

  err = cpc_gpio_multicast_exit("Kernel Driver is no longer loaded");
  if (err != 0) {
    pr_err("%s: cpc_gpio_multicast_exit failed: %d\n", __func__,
           err);
  }

  err = genl_unregister_family(&cpc_gpio_genl_family);
  if (err != 0) {
    pr_err("%s: genl_unregister_family failed: %d\n", __func__,
           err);
  }

  platform_driver_unregister(&cpc_gpio_driver);

  list_for_each_entry_safe(list_item, list_item_tmp, &cpc_gpio_chip_list, list)
  {
    cpc_unregister_chip(&list_item->chip);
  }

  pr_info("%s: Driver API v%d.%d.%d, GENL API v%d\n", __func__, CPC_GPIO_API_VERSION_MAJOR,
          CPC_GPIO_API_VERSION_MINOR,
          CPC_GPIO_API_VERSION_PATCH,
          CPC_GPIO_GENL_VERSION);
}

module_init(cpc_gpio_init);
module_exit(cpc_gpio_exit);

MODULE_AUTHOR("Silicon Labs");
MODULE_DESCRIPTION("CPC GPIO Driver");
MODULE_LICENSE("GPL v2");
