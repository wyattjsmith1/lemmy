use crate::{
  objects::{community::ApubCommunity, post::ApubPost},
  protocol::collections::group_featured::GroupFeatured,
};
use activitypub_federation::{
  config::Data,
  kinds::collection::OrderedCollectionType,
  protocol::verification::verify_domains_match,
  traits::{ActivityHandler, ApubCollection, ApubObject},
};
use futures::future::{join_all, try_join_all};
use lemmy_api_common::{context::LemmyContext, utils::generate_featured_url};
use lemmy_db_schema::utils::FETCH_LIMIT_MAX;
use lemmy_utils::error::LemmyError;
use url::Url;

#[derive(Clone, Debug)]
pub(crate) struct ApubCommunityFeatured(Vec<ApubPost>);

#[async_trait::async_trait]
impl ApubCollection for ApubCommunityFeatured {
  type Owner = ApubCommunity;
  type DataType = LemmyContext;
  type ApubType = GroupFeatured;
  type Error = LemmyError;

  async fn into_apub(
    self,
    owner: Self::Owner,
    data: &Data<Self::DataType>,
  ) -> Result<Self::ApubType, Self::Error> {
    let ordered_items = try_join_all(self.0.into_iter().map(|p| p.into_apub(data))).await?;
    Ok(GroupFeatured {
      r#type: OrderedCollectionType::OrderedCollection,
      id: generate_featured_url(&owner.actor_id)?.into(),
      total_items: ordered_items.len() as i32,
      ordered_items,
    })
  }

  async fn verify(
    apub: &Self::ApubType,
    _owner: Self::Owner,
    expected_domain: &Url,
    _data: &Data<Self::DataType>,
  ) -> Result<(), Self::Error> {
    verify_domains_match(expected_domain, &apub.id)?;
    Ok(())
  }

  async fn from_apub(
    apub: Self::ApubType,
    _owner: Self::Owner,
    data: &Data<Self::DataType>,
  ) -> Result<Self, Self::Error>
  where
    Self: Sized,
  {
    let mut posts = apub.ordered_items;
    if posts.len() as i64 > FETCH_LIMIT_MAX {
      posts = posts
        .get(0..(FETCH_LIMIT_MAX as usize))
        .unwrap_or_default()
        .to_vec();
    }

    // We intentionally ignore errors here. This is because the outbox might contain posts from old
    // Lemmy versions, or from other software which we cant parse. In that case, we simply skip the
    // item and only parse the ones that work.
    // process items in parallel, to avoid long delay from fetch_site_metadata() and other processing
    join_all(posts.into_iter().map(|post| {
      async {
        // use separate request counter for each item, otherwise there will be problems with
        // parallel processing
        let verify = post.verify(data).await;
        if verify.is_ok() {
          post.receive(data).await.ok();
        }
      }
    }))
    .await;

    // This return value is unused, so just set an empty vec
    Ok(ApubCommunityFeatured(Vec::new()))
  }
}
